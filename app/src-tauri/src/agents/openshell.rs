//! OpenShell (NVIDIA/OpenShell): another place a local ACP agent can run. The
//! harness's adapter runs inside an OpenShell sandbox, on a local or a remote
//! gateway, and its stdio comes back through `openshell sandbox exec`, which
//! the agent host runs like any adapter. Approvals, the transcript, Stop and
//! resuming after a restart are the local session's (`acp.rs`).
//!
//! The OpenShell CLI owns the gateways, their sign-in and the providers that
//! hold credentials; Alethiometer only runs it (`agents.openshell.command`).
//! Built from OpenShell's source at `67adcf1` (2026-10-08) and its docs; not
//! run against a gateway here.
//!
//! The sandbox is the boundary, so the agent's own permission prompts are
//! turned off inside it: Claude Code runs in its `auto` mode (it decides what
//! to allow, and anything it still asks comes to you); any other agent is
//! switched to a mode that skips its checks if it offers one, and its
//! permission requests are answered with its own allow option. Questions
//! still come to you, and so does what the sandbox itself blocks: OpenShell's
//! drafted network rules (`rule get --status pending`) are approvals here.
//!
//! What the agent changed comes back through `sync` (download, then a
//! three-way check against what was uploaded); a sandbox Alethiometer made is
//! deleted when its session is archived (`openshell.delete_sandbox`, outbox).

pub mod sync;

use super::model::{Kind, PermOption, Request, Row, Sandbox, Status};
use crate::error::{msg, Result};
use crate::settings::Harness;
use crate::state::App;
use serde::Serialize;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// Listing gateways or sandboxes, starting a stopped one, asking its working directory.
const QUICK: Duration = Duration::from_secs(120);
/// Creating a sandbox: the gateway may pull the image first, then the directory is uploaded.
const CREATE: Duration = Duration::from_secs(20 * 60);
/// How often a running sandbox's drafted network rules are read.
const RULES_EVERY: Duration = Duration::from_secs(10);
/// The outbox op that deletes a sandbox Alethiometer made (`sync/connectors/agents.rs`).
pub const DELETE_OP: &str = "openshell.delete_sandbox";

/// Where the CLI is, on the login shell's PATH.
pub fn cli(command: &str) -> Option<std::path::PathBuf> {
    super::acp::which(command, &super::acp::login_path())
}

/// `-g <gateway>`, unless it's the CLI's active one.
fn gateway_flag(gateway: &str) -> Vec<String> {
    if gateway.is_empty() {
        vec![]
    } else {
        vec!["-g".into(), gateway.into()]
    }
}

/// A new sandbox, from the session's harness settings and the form.
#[derive(Debug, Clone, Default)]
pub struct NewSandbox {
    pub gateway: String,
    pub name: String,
    pub from: String,
    pub providers: Vec<String>,
    pub policy: String,
    /// The local directory uploaded into its working directory (`.gitignore` applies).
    pub upload: String,
}

/// `sandbox create`, kept running with no command of its own (a login shell)
/// so the adapter can be started in it with `exec`. `--upload` can't be
/// combined with a command yet, which suits this. Missing providers are an
/// error, never created from local credentials.
pub fn create_args(n: &NewSandbox) -> Vec<String> {
    let mut a = gateway_flag(&n.gateway);
    a.extend(["sandbox", "create", "--name", &n.name].map(String::from));
    if !n.from.is_empty() {
        a.extend(["--from".into(), n.from.clone()]);
    }
    for p in n.providers.iter().filter(|p| !p.trim().is_empty()) {
        a.extend(["--provider".into(), p.trim().to_owned()]);
    }
    if !n.policy.is_empty() {
        a.extend(["--policy".into(), super::acp::expand_home(&n.policy)]);
    }
    a.extend(["--upload".into(), n.upload.clone()]);
    a.extend(["--detach", "--no-auto-providers", "--no-tty", "-o", "json"].map(String::from));
    a
}

/// The adapter inside the sandbox: the harness's `openshell` command, else its own.
pub fn adapter(h: &Harness) -> (String, Vec<String>, std::collections::BTreeMap<String, String>) {
    match &h.openshell {
        Some(o) if !o.command.is_empty() => (o.command.clone(), o.args.clone(), o.env.clone()),
        Some(o) => (h.command.clone(), h.args.clone(), o.env.clone()),
        None => (h.command.clone(), h.args.clone(), Default::default()),
    }
}

/// `sandbox exec` running the adapter in `workdir`, its stdio piped (no TTY:
/// ACP is one JSON message per line). It runs through the sandbox user's
/// login shell, so an image's profile can put the adapter on PATH. No
/// timeout: the exec lasts as long as the agent.
pub fn exec_args(sb: &Sandbox, workdir: &str, h: &Harness) -> Vec<String> {
    let (cmd, args, env) = adapter(h);
    let mut a = gateway_flag(&sb.gateway);
    a.extend(["sandbox", "exec", "-n", &sb.name, "--no-tty", "--timeout", "0"].map(String::from));
    if !workdir.is_empty() {
        a.extend(["--workdir".into(), workdir.to_owned()]);
    }
    for (k, v) in env {
        a.extend(["--env".into(), format!("{k}={v}")]);
    }
    a.push("--".into());
    a.push(cmd);
    a.extend(args);
    a
}

/// A shell in the sandbox at `workdir`, for a terminal.
pub fn terminal_command(command: &str, sb: &Sandbox, workdir: &str) -> String {
    let mut a = vec![command.to_owned()];
    a.extend(gateway_flag(&sb.gateway));
    a.extend(["sandbox", "exec", "-n", &sb.name, "--tty"].map(String::from));
    if !workdir.is_empty() {
        a.extend(["--workdir".into(), workdir.to_owned()]);
    }
    a.extend(["--", "bash", "-l"].map(String::from));
    shlex::try_join(a.iter().map(String::as_str)).unwrap_or_else(|_| a.join(" "))
}

/// Run the CLI and return its stdout; a failure carries the end of its stderr.
fn run(command: &str, args: &[String], timeout: Duration) -> Result<String> {
    let path = super::acp::login_path();
    let exe = super::acp::which(command, &path)
        .ok_or_else(|| msg(format!("{command} isn't on your PATH (agents.openshell.command)")))?;
    let mut c = Command::new(exe);
    c.args(args).env("PATH", path).env("NO_COLOR", "1").stdin(Stdio::null());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(c.output());
    });
    let out = rx
        .recv_timeout(timeout)
        .map_err(|_| {
            msg(format!(
                "openshell {} didn't finish in {} s",
                args.iter()
                    .find(|a| !a.starts_with('-'))
                    .map(String::as_str)
                    .unwrap_or(""),
                timeout.as_secs()
            ))
        })?
        .map_err(|e| msg(e.to_string()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = err.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(3)..].join(" · ");
    Err(msg(if tail.is_empty() {
        format!("openshell exited with {}", out.status)
    } else {
        tail
    }))
}

/// The gateways the CLI knows (`gateway list -o json`): name, endpoint, local or remote, and which is active.
pub fn gateways(command: &str) -> Result<Value> {
    let out = run(command, &["gateway", "list", "-o", "json"].map(String::from), QUICK)?;
    Ok(json!(parse_gateways(&out)))
}

pub fn parse_gateways(out: &str) -> Vec<Value> {
    let list: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    list.iter()
        .filter(|g| !s(g, "name").is_empty())
        .map(|g| {
            json!({
                "name": s(g, "name"),
                "endpoint": s(g, "endpoint"),
                "remote": g.get("is_remote").and_then(Value::as_bool).unwrap_or(false) || s(g, "type") == "remote",
                "active": g.get("active").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// A gateway's sandboxes (`sandbox list -o json`): name and phase.
pub fn sandboxes(command: &str, gateway: &str) -> Result<Value> {
    let mut a = gateway_flag(gateway);
    a.extend(["sandbox", "list", "-o", "json"].map(String::from));
    Ok(json!(parse_sandboxes(&run(command, &a, QUICK)?)))
}

pub fn parse_sandboxes(out: &str) -> Vec<Value> {
    let list: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    list.iter()
        .filter(|b| !s(b, "name").is_empty())
        .map(|b| json!({ "name": s(b, "name"), "phase": s(b, "phase") }))
        .collect()
}

/// Create the sandbox and upload the directory; returns where it landed
/// inside: the sandbox's working directory, then the directory's own name
/// (OpenShell keeps it, as `cp -r` does).
pub fn create(command: &str, n: &NewSandbox) -> Result<String> {
    run(command, &create_args(n), CREATE)?;
    let home = working_dir(command, &n.gateway, &n.name)?;
    let base = std::path::Path::new(n.upload.trim_end_matches('/'))
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(if base.is_empty() {
        home
    } else {
        format!("{}/{base}", home.trim_end_matches('/'))
    })
}

/// The sandbox's working directory, where uploads land.
fn working_dir(command: &str, gateway: &str, name: &str) -> Result<String> {
    let mut a = gateway_flag(gateway);
    a.extend(
        [
            "sandbox",
            "exec",
            "-n",
            name,
            "--no-tty",
            "--no-login-shell",
            "--",
            "pwd",
        ]
        .map(String::from),
    );
    let out = run(command, &a, QUICK)?;
    out.lines()
        .map(str::trim)
        .rfind(|l| l.starts_with('/'))
        .map(str::to_owned)
        .ok_or_else(|| msg("couldn't tell the sandbox's working directory"))
}

/// Start the sandbox if it was stopped (a no-op when it's running), before the adapter is started in it again.
pub fn ensure_started(command: &str, sb: &Sandbox) -> Result<()> {
    let mut a = gateway_flag(&sb.gateway);
    a.extend(["sandbox", "start", &sb.name].map(String::from));
    run(command, &a, QUICK).map(|_| ())
}

/// A sandbox name from the session id: lowercase letters, digits and hyphens, at most 19 (OpenShell's routable limit).
pub fn sandbox_name(session_id: &str) -> String {
    let hex: String = session_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_lowercase();
    format!("aleth-{}", &hex[hex.len().saturating_sub(8)..])
}

/// `sandbox delete`: the sandbox, its files and its injected credentials go. An absent one is a success.
pub fn delete_sandbox(command: &str, gateway: &str, name: &str) -> Result<String> {
    let mut a = gateway_flag(gateway);
    a.extend(["sandbox", "delete", name].map(String::from));
    run(command, &a, QUICK)
}

/// `sandbox download`: the contents of `from` (a directory inside the sandbox) into `to`.
pub fn download(command: &str, sb: &Sandbox, from: &str, to: &std::path::Path) -> Result<()> {
    let mut a = gateway_flag(&sb.gateway);
    a.extend([
        "sandbox".into(),
        "download".into(),
        sb.name.clone(),
        from.to_owned(),
        to.display().to_string(),
    ]);
    run(command, &a, CREATE).map(|_| ())
}

/// A network rule OpenShell drafted after the sandbox blocked a connection
/// (its policy advisor), waiting for someone to approve or reject it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Rule {
    pub chunk: String,
    pub status: String,
    pub rule: String,
    /// The program that tried to connect.
    pub binary: String,
    pub confidence: String,
    pub rationale: String,
    /// The prover's or advisor's warning: new access with credentials, a new API method.
    pub security: String,
    pub prover: String,
    /// "api.github.com:443 [L7 rest, …]".
    pub endpoints: String,
    pub binaries: String,
    pub hits: String,
}

/// `rule get`'s blocks ("  Chunk: …", "  Status: …", one blank line between).
/// It has no JSON output, so this reads the labels openshell-cli's
/// `sandbox_draft_get` prints (plain text when piped, and `NO_COLOR` is set).
pub fn parse_rules(out: &str) -> Vec<Rule> {
    let mut rules: Vec<Rule> = vec![];
    for line in out.lines() {
        let Some((k, v)) = line.trim().split_once(':') else {
            continue;
        };
        let v = v.trim().to_owned();
        if k == "Chunk" {
            rules.push(Rule {
                chunk: v,
                ..Default::default()
            });
            continue;
        }
        let Some(r) = rules.last_mut() else { continue };
        match k {
            "Status" => r.status = v,
            "Rule" => r.rule = v,
            "Binary" => r.binary = v,
            "Confidence" => r.confidence = v,
            "Rationale" => r.rationale = v,
            "Security" => r.security = v,
            "Prover" => r.prover = v,
            "Endpoints" => r.endpoints = v,
            "Binaries" => r.binaries = v,
            "Hits" => r.hits = v,
            _ => {}
        }
    }
    rules.retain(|r| !r.chunk.is_empty());
    rules
}

/// The rules waiting on someone for this sandbox.
pub fn pending_rules(command: &str, sb: &Sandbox) -> Result<Vec<Rule>> {
    let mut a = gateway_flag(&sb.gateway);
    a.extend(["rule", "get", &sb.name, "--status", "pending"].map(String::from));
    Ok(parse_rules(&run(command, &a, QUICK)?)
        .into_iter()
        .filter(|r| r.status.is_empty() || r.status == "pending")
        .collect())
}

/// Approve a drafted rule (it hot-reloads into the sandbox), or reject it with a reason.
pub fn decide_rule(command: &str, sb: &Sandbox, chunk: &str, approve: bool, reason: &str) -> Result<()> {
    let mut a = gateway_flag(&sb.gateway);
    if approve {
        a.extend(["rule", "approve", &sb.name, "--chunk-id", chunk].map(String::from));
    } else {
        a.extend(["rule", "reject", &sb.name, "--chunk-id", chunk, "--reason", reason].map(String::from));
    }
    run(command, &a, QUICK).map(|_| ())
}

/// A rule's request id: `osr:<session>:<chunk>`.
fn rule_request(session: &str, chunk: &str) -> String {
    format!("osr:{session}:{chunk}")
}

/// A drafted rule as an approval in its session: Approve or Reject, nothing parsed or explained.
pub fn rule_as_request(session: &str, sb: &Sandbox, cwd: &str, r: &Rule, at: &str) -> Request {
    let what = if r.endpoints.is_empty() {
        r.rule.clone()
    } else {
        r.endpoints.clone()
    };
    let mut message = vec![];
    if !r.binary.is_empty() || !r.binaries.is_empty() {
        message.push(format!(
            "from {}",
            if r.binaries.is_empty() { &r.binary } else { &r.binaries }
        ));
    }
    for (label, v) in [
        ("why", &r.rationale),
        ("security", &r.security),
        ("prover", &r.prover),
        ("confidence", &r.confidence),
        ("hits", &r.hits),
    ] {
        if !v.is_empty() {
            message.push(format!("{label}: {v}"));
        }
    }
    let opt = |id: &str, name: &str, kind: &str, key: &str| PermOption {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        key: key.into(),
    };
    Request {
        id: rule_request(session, &r.chunk),
        session_id: session.to_owned(),
        thread: super::session_thread(session),
        kind: "approval".into(),
        at: at.to_owned(),
        title: "Allow network access from the sandbox".into(),
        tool: format!("network · sandbox {}", sb.name),
        cwd: cwd.to_owned(),
        command: what,
        message: message.join("\n"),
        options: vec![
            opt("approve", "Approve rule", "allow_once", "1"),
            opt("reject", "Reject", "reject_once", "2"),
        ],
        tag: if r.security.is_empty() {
            "network".into()
        } else {
            "network · flagged".into()
        },
        ..Default::default()
    }
}

/// Bring a session's network approvals in line with the rules pending now:
/// new ones pinned (with a row in its thread, reused after a restart), ones
/// decided elsewhere (`openshell rule approve`, the TUI) taken down.
pub fn reconcile_rules(app: &App, session: &str, rules: &[Rule]) {
    let key = super::session_thread(session);
    if app.agents.store.ensure_loaded(app.store(), &key).is_err() {
        return;
    }
    let at = super::now(app);
    let mut i = app.agents.store.lock();
    let Some(s) = i.sessions.get(session).cloned() else {
        return;
    };
    let Some(sb) = s.sandbox.clone() else { return };
    let want: Vec<Request> = rules
        .iter()
        .map(|r| rule_as_request(session, &sb, &s.cwd, r, &at))
        .collect();
    // Stale: pinned now, or left waiting in the stored thread by an earlier run (requests aren't stored).
    let mine = format!("osr:{session}:");
    let pinned = i.requests.keys().filter(|id| id.starts_with(&mine)).cloned();
    let stored = i
        .threads
        .get(&key)
        .map(|t| {
            t.rows
                .iter()
                .filter(|r| r.request.is_some() && r.ext.starts_with(&mine))
                .map(|r| r.ext.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut stale: Vec<String> = pinned
        .chain(stored)
        .filter(|id| !want.iter().any(|w| &w.id == id))
        .collect();
    stale.sort();
    stale.dedup();
    for id in stale {
        i.remove_request(&id);
        if let Some(row) = i.find_row(&key, |r| r.ext == id).map(|r| r.id) {
            i.update_row(&key, row, |r| {
                r.kind = Kind::Tool;
                r.label.clear();
                r.request = None;
                r.note = "decided outside Alethiometer".into();
            });
        }
    }
    for mut req in want {
        if let Some(old) = i.requests.get(&req.id) {
            req.at = old.at.clone();
        } else if i.find_row(&key, |r| r.ext == req.id && r.request.is_some()).is_none() {
            let tool = super::model::Tool {
                name: "network".into(),
                detail: req.command.clone(),
                status: "pending".into(),
                ..Default::default()
            };
            i.push_row(
                &key,
                Row {
                    at: at.clone(),
                    kind: Kind::Approval,
                    label: "network access".into(),
                    text: req.command.clone(),
                    tool: Some(tool),
                    request: Some(req.id.clone()),
                    ext: req.id.clone(),
                    ..Default::default()
                },
            );
        }
        i.put_request(req);
    }
}

/// Approve or reject a drafted rule from its approval. The CLI runs with nothing held.
pub fn respond_rule(app: &App, req_id: &str, option: &str, reason: Option<&str>) -> Result<()> {
    let req = app
        .agents
        .store
        .lock()
        .requests
        .get(req_id)
        .cloned()
        .ok_or_else(|| msg("that request is gone"))?;
    let opt = req
        .options
        .iter()
        .find(|o| o.id == option || o.key == option)
        .cloned()
        .ok_or_else(|| msg("not one of its options"))?;
    let chunk = req_id
        .strip_prefix(&format!("osr:{}:", req.session_id))
        .ok_or_else(|| msg("not a network rule"))?
        .to_owned();
    let sb = app
        .agents
        .store
        .lock()
        .sessions
        .get(&req.session_id)
        .and_then(|s| s.sandbox.clone())
        .ok_or_else(|| msg("its sandbox is gone"))?;
    let approve = opt.id == "approve";
    decide_rule(
        &app.settings().agents.openshell.command,
        &sb,
        &chunk,
        approve,
        reason.unwrap_or("").trim(),
    )?;
    let hm = app.clock.local_now().strftime("%H:%M").to_string();
    let key = req.thread.clone();
    let mut i = app.agents.store.lock();
    i.remove_request(req_id);
    if let Some(row) = i.find_row(&key, |r| r.ext == req_id).map(|r| r.id) {
        // Answered, it reads as a resolved approval does: a tool row with the verdict.
        i.update_row(&key, row, |r| {
            r.kind = Kind::Tool;
            r.label.clear();
            r.request = None;
            if let Some(t) = r.tool.as_mut() {
                t.status = if approve { "completed".into() } else { "failed".into() };
            }
            r.note = if approve {
                format!("approved at {hm}; the sandbox lets it through now")
            } else {
                format!("rejected at {hm}")
            };
        });
    }
    Ok(())
}

/// Read running sandboxes' drafted rules every 10 s (one session per sandbox: the most recently active).
pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("agents-openshell".into())
        .spawn(move || {
            let mut last_err: std::collections::HashMap<String, String> = Default::default();
            loop {
                std::thread::sleep(RULES_EVERY);
                let command = app.settings().agents.openshell.command.clone();
                let mut targets: Vec<(String, Sandbox, String)> = {
                    let i = app.agents.store.lock();
                    i.sessions
                        .values()
                        .filter(|s| {
                            s.source == "acp"
                                && s.parent_id.is_none()
                                && (s.live || matches!(s.status, Status::Running | Status::Waiting))
                        })
                        .filter_map(|s| Some((s.id.clone(), s.sandbox.clone()?, s.active_at.clone())))
                        .collect()
                };
                if targets.is_empty() || cli(&command).is_none() {
                    continue;
                }
                targets.sort_by(|a, b| b.2.cmp(&a.2));
                let mut seen = std::collections::HashSet::new();
                for (session, sb, _) in targets {
                    if !seen.insert((sb.gateway.clone(), sb.name.clone())) {
                        continue;
                    }
                    match pending_rules(&command, &sb) {
                        Ok(rules) => {
                            last_err.remove(&sb.name);
                            reconcile_rules(&app, &session, &rules);
                        }
                        Err(e) => {
                            if last_err.insert(sb.name.clone(), e.to_string()).as_deref() != Some(&e.to_string()) {
                                eprintln!("agents: couldn't read sandbox {}'s network rules: {e}", sb.name);
                            }
                        }
                    }
                }
            }
        })
        .ok();
}

/// Record what was just uploaded into a new sandbox, the base changes are brought back against.
pub fn record_upload(app: &App, sb: &Sandbox) -> Result<()> {
    let m = sync::snapshot(std::path::Path::new(&sb.upload))?;
    sync::save(&sync::manifest_path(&app.data_dir, sb), &m)
}

/// The session's sandbox, if its changes can come back: one Alethiometer made from a directory here.
fn syncable(app: &App, session: &str) -> Result<(super::model::Session, Sandbox)> {
    let s = app
        .agents
        .store
        .lock()
        .sessions
        .get(session)
        .cloned()
        .ok_or_else(|| msg("no such session"))?;
    let sb = s.sandbox.clone().ok_or_else(|| msg("It doesn't run in a sandbox"))?;
    if sb.upload.is_empty() {
        return Err(msg(
            "Only a sandbox made here, from a directory, brings changes back; this one was already there",
        ));
    }
    if !std::path::Path::new(&sb.upload).is_dir() {
        return Err(msg(format!("{} isn't here any more", super::acp::tilde(&sb.upload))));
    }
    Ok((s, sb))
}

fn plan_json(sb: &Sandbox, plan: &sync::Plan, ready: bool) -> Value {
    json!({ "sandbox": sb.name, "local": super::acp::tilde(&sb.upload), "changes": plan.changes, "conflicts": plan.conflicts, "same": plan.same, "ready": ready })
}

/// Download the sandbox's directory and work out what would come back (nothing is touched here yet).
/// With nothing to bring back and nothing in conflict, the session counts as brought back.
pub fn preview(app: &App, session: &str) -> Result<Value> {
    let (s, sb) = syncable(app, session)?;
    sync::discard(session);
    let dir =
        app.data_dir
            .join("openshell-sync")
            .join("downloads")
            .join(format!("{}-{}", sb.name, super::acp::now_ms_pub()));
    std::fs::create_dir_all(&dir).map_err(|e| msg(e.to_string()))?;
    let local = std::path::Path::new(&sb.upload);
    let planned = download(&app.settings().agents.openshell.command, &sb, &s.cwd, &dir)
        .and_then(|_| sync::plan(&sync::load(&sync::manifest_path(&app.data_dir, &sb)), local, &dir));
    let plan = match planned {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    if !plan.changes.is_empty() {
        let out = plan_json(&sb, &plan, true);
        sync::previews().lock().unwrap_or_else(|e| e.into_inner()).insert(
            session.to_owned(),
            sync::Preview {
                dir,
                plan,
                turns: s.stats.turns,
            },
        );
        return Ok(out);
    }
    // Nothing to copy: settle what's the same on both sides, and say so.
    let path = sync::manifest_path(&app.data_dir, &sb);
    let (_, conflicts, next) = sync::apply(&plan, &sync::load(&path), local, &dir, false);
    let _ = std::fs::remove_dir_all(&dir);
    sync::save(&path, &next)?;
    if conflicts.is_empty() {
        app.agents.store.lock().update_session(session, |x| {
            if let Some(b) = x.sandbox.as_mut() {
                b.synced_turns = s.stats.turns;
            }
        });
    }
    Ok(plan_json(&sb, &plan, false))
}

/// Copy back what the preview listed (with `force`, the conflicts too, the
/// sandbox's version winning), note it in the session, and drop the download.
pub fn copy_back(app: &App, session: &str, force: bool) -> Result<Value> {
    let (_, sb) = syncable(app, session)?;
    let p = sync::previews()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(session)
        .ok_or_else(|| msg("Look at the changes first"))?;
    let path = sync::manifest_path(&app.data_dir, &sb);
    let (done, conflicts, next) = sync::apply(
        &p.plan,
        &sync::load(&path),
        std::path::Path::new(&sb.upload),
        &p.dir,
        force,
    );
    let _ = std::fs::remove_dir_all(&p.dir);
    sync::save(&path, &next)?;
    let ts = super::now(app);
    let files = |n: usize| format!("{n} file{}", if n == 1 { "" } else { "s" });
    let mut text = format!(
        "Brought back {} from sandbox {} into {}",
        files(done.len()),
        sb.name,
        super::acp::tilde(&sb.upload)
    );
    if !conflicts.is_empty() {
        let names: Vec<&str> = conflicts.iter().take(5).map(|c| c.path.as_str()).collect();
        text.push_str(&format!(
            " · {} left as they are here, changed on both sides: {}{}",
            files(conflicts.len()),
            names.join(", "),
            if conflicts.len() > 5 { ", …" } else { "" }
        ));
    }
    let mut i = app.agents.store.lock();
    i.push_row(
        &super::session_thread(session),
        Row {
            at: ts,
            kind: Kind::State,
            text,
            ..Default::default()
        },
    );
    if conflicts.is_empty() {
        i.update_session(session, |x| {
            if let Some(b) = x.sandbox.as_mut() {
                b.synced_turns = p.turns;
            }
        });
    }
    Ok(json!({ "applied": done, "conflicts": conflicts }))
}

/// Claude Code's adapter (claude-agent-acp), from `initialize`'s `agentInfo`.
pub fn is_claude(init: &Value) -> bool {
    init.pointer("/agentInfo/name")
        .and_then(Value::as_str)
        .is_some_and(|n| n.to_lowercase().contains("claude"))
}

/// The mode a sandboxed session switches to, if it isn't in it already:
/// Claude Code's `auto` (claude-agent-acp falls back to accept-edits, with a
/// notice, for a model without it); for any other agent, one that skips
/// permission checks (`bypassPermissions`, codex-acp's `full-access`, `yolo`).
pub fn mode(init: &Value, opened: &Value) -> Option<String> {
    let modes: Vec<&str> = opened
        .pointer("/modes/availableModes")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .collect();
    let current = opened.pointer("/modes/currentModeId").and_then(Value::as_str);
    let want: &[&str] = if is_claude(init) {
        &["auto"]
    } else {
        &["bypassPermissions", "full-access", "full_access", "yolo"]
    };
    let m = want.iter().find(|w| modes.contains(w))?;
    (current != Some(*m)).then(|| (*m).to_owned())
}

/// The option to answer a permission request with for an agent whose checks
/// are off, or None when it's for you. A request about a tool call the agent
/// announced is a gate. One about no known call is a prompt an extension
/// raised (pi-acp turns an extension's `select` or `confirm` into one): a gate
/// only while a tool call waits to run, since pi's permission extensions ask
/// from the `tool_call` hook, before the call starts (pi-acp has shown it as
/// pending by then); otherwise it's a question, and it comes to you.
pub fn auto_allow(options: &[PermOption], known_call: bool, call_pending: bool) -> Option<&PermOption> {
    (known_call || call_pending).then(|| allow_option(options)).flatten()
}

/// The option that allows a permission request, for agents whose checks are
/// off in the sandbox. By kind when the kinds tell allow from deny (allow
/// once before always, so nothing is saved as a rule); otherwise by name:
/// pi-acp sends every choice of an extension's `select` (a permission
/// extension's "Allow" / "Deny") as `allow_once`. None when nothing reads as
/// an allow: the request is left to you.
pub fn allow_option(options: &[PermOption]) -> Option<&PermOption> {
    if options.iter().any(|o| o.kind.starts_with("reject")) {
        return options
            .iter()
            .find(|o| o.kind == "allow_once")
            .or_else(|| options.iter().find(|o| o.kind == "allow_always"));
    }
    let yes = regex::Regex::new(r"^(yes|allow|approve|ok|okay|proceed|continue|run)\b").ok()?;
    let no = regex::Regex::new(r"\b(no|not|never|deny|reject|block|cancel|abort|don't|dont)\b").ok()?;
    options.iter().find(|o| {
        let n = o.name.trim().to_lowercase();
        yes.is_match(&n) && !no.is_match(&n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::HarnessSandbox;

    fn opt(id: &str, name: &str, kind: &str) -> PermOption {
        PermOption {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            key: String::new(),
        }
    }

    fn harness(openshell: Option<HarnessSandbox>) -> Harness {
        Harness {
            key: "claude".into(),
            label: "Claude Code".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@agentclientprotocol/claude-agent-acp".into()],
            env: Default::default(),
            cache_ttl_min: None,
            openshell,
        }
    }

    #[test]
    fn claude_runs_in_auto_mode() {
        let init = json!({ "agentInfo": { "name": "@agentclientprotocol/claude-agent-acp" } });
        let opened = json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "acceptEdits" }, { "id": "plan" }, { "id": "auto" }, { "id": "bypassPermissions" }] } });
        assert_eq!(
            mode(&init, &opened).as_deref(),
            Some("auto"),
            "auto, not bypass, even when bypass is offered"
        );
        let already = json!({ "modes": { "currentModeId": "auto", "availableModes": [{ "id": "auto" }] } });
        assert_eq!(mode(&init, &already), None);
        assert!(is_claude(&init));
    }

    #[test]
    fn other_agents_skip_their_checks() {
        let codex = json!({ "agentInfo": { "name": "codex-acp" } });
        let opened = json!({ "modes": { "currentModeId": "auto", "availableModes": [{ "id": "read-only" }, { "id": "auto" }, { "id": "full-access" }] } });
        assert_eq!(
            mode(&codex, &opened).as_deref(),
            Some("full-access"),
            "codex's auto still asks"
        );
        let pi = json!({ "agentInfo": { "name": "pi-acp" } });
        let levels =
            json!({ "modes": { "currentModeId": "medium", "availableModes": [{ "id": "low" }, { "id": "medium" }] } });
        assert_eq!(mode(&pi, &levels), None, "pi's modes are thinking levels");
        assert!(!is_claude(&pi));
        let dont_ask = json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "dontAsk" }] } });
        assert_eq!(mode(&pi, &dont_ask), None, "dontAsk denies what isn't allowed already");
    }

    #[test]
    fn the_allow_option_by_kind() {
        let claude = [
            opt("always", "Yes, and don't ask again for git", "allow_always"),
            opt("once", "Yes", "allow_once"),
            opt("no", "No", "reject_once"),
        ];
        assert_eq!(
            allow_option(&claude).map(|o| o.id.as_str()),
            Some("once"),
            "once, so no rule is saved"
        );
        let only_always = [opt("a", "Always", "allow_always"), opt("n", "No", "reject_once")];
        assert_eq!(allow_option(&only_always).map(|o| o.id.as_str()), Some("a"));
        let pi_confirm = [opt("yes", "Yes", "allow_once"), opt("no", "No", "reject_once")];
        assert_eq!(allow_option(&pi_confirm).map(|o| o.id.as_str()), Some("yes"));
    }

    #[test]
    fn the_allow_option_by_name_when_kinds_dont_tell() {
        // pi-acp: an extension's select, every choice allow_once.
        let gate = [
            opt("choice-0", "Deny", "allow_once"),
            opt("choice-1", "Allow once", "allow_once"),
            opt("choice-2", "Allow always", "allow_once"),
        ];
        assert_eq!(allow_option(&gate).map(|o| o.id.as_str()), Some("choice-1"));
        let not = [
            opt("choice-0", "Don't allow", "allow_once"),
            opt("choice-1", "No, provide reason", "allow_once"),
        ];
        assert_eq!(allow_option(&not), None);
        let question = [
            opt("choice-0", "main", "allow_once"),
            opt("choice-1", "dev", "allow_once"),
        ];
        assert_eq!(
            allow_option(&question),
            None,
            "a select that isn't a gate is left to you"
        );
    }

    #[test]
    fn a_question_isnt_answered_for_you() {
        let yes_no = [
            opt("choice-0", "Yes", "allow_once"),
            opt("choice-1", "No", "allow_once"),
        ];
        assert_eq!(
            auto_allow(&yes_no, true, false).map(|o| o.id.as_str()),
            Some("choice-0"),
            "a gate on a call the agent announced"
        );
        assert_eq!(
            auto_allow(&yes_no, false, true).map(|o| o.id.as_str()),
            Some("choice-0"),
            "pi's permission gate: a call is waiting to run"
        );
        assert_eq!(
            auto_allow(&yes_no, false, false),
            None,
            "an extension asking with no call waiting: a question"
        );
    }

    /// `rule get --status pending` as openshell-cli's sandbox_draft_get prints it, piped (no colour).
    const RULES: &str = "Network Rules:  (version 4, 2 chunks)

  Chunk: c-7f1
  Status: pending
  Rule: allow-github-api
  Binary: /usr/bin/git
  Confidence: 92%
  Rationale: git fetch from github.com was denied 3 times
  Prover: no new credentialed access
  Endpoints: github.com:443 [L4]
  Binaries: /usr/bin/git
  Hits: 3 (first seen 2026-10-08 05:36:19, last seen 2026-10-08 05:37:02)

  Chunk: c-7f2
  Status: pending
  Rule: allow-pypi
  Confidence: 60%
  Rationale: pip install reached pypi.org
  Security: grants a new host with credentials
  Endpoints: pypi.org:443 [L7 rest, GET /simple/**]

";

    #[test]
    fn drafted_rules_are_read_from_the_cli() {
        let rules = parse_rules(RULES);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].chunk, "c-7f1");
        assert_eq!(rules[0].endpoints, "github.com:443 [L4]");
        assert_eq!(
            rules[0].hits, "3 (first seen 2026-10-08 05:36:19, last seen 2026-10-08 05:37:02)",
            "a value with colons in it"
        );
        assert_eq!(rules[1].security, "grants a new host with credentials");
        assert!(parse_rules("No network rules for sandbox 'aleth-1'\n").is_empty());

        let sb = Sandbox {
            name: "aleth-1".into(),
            ..Default::default()
        };
        let req = rule_as_request("acp-1", &sb, "/sandbox/app", &rules[1], "2026-10-08T05:40:00Z");
        assert_eq!(req.id, "osr:acp-1:c-7f2");
        assert_eq!(req.kind, "approval");
        assert_eq!(req.command, "pypi.org:443 [L7 rest, GET /simple/**]");
        assert_eq!(req.tag, "network · flagged");
        assert!(req.message.contains("security: grants a new host with credentials"));
        assert_eq!(
            req.options
                .iter()
                .map(|o| (o.id.as_str(), o.kind.as_str()))
                .collect::<Vec<_>>(),
            [("approve", "allow_once"), ("reject", "reject_once")]
        );
    }

    #[test]
    fn exec_runs_the_sandbox_adapter() {
        let sb = Sandbox {
            gateway: "remote-k8s".into(),
            name: "aleth-1a2b3c4d".into(),
            ..Default::default()
        };
        let own = exec_args(&sb, "/sandbox/alethiometer", &harness(None));
        assert_eq!(
            own,
            [
                "-g",
                "remote-k8s",
                "sandbox",
                "exec",
                "-n",
                "aleth-1a2b3c4d",
                "--no-tty",
                "--timeout",
                "0",
                "--workdir",
                "/sandbox/alethiometer",
                "--",
                "npx",
                "-y",
                "@agentclientprotocol/claude-agent-acp"
            ]
            .map(String::from)
            .to_vec()
        );
        let image = HarnessSandbox {
            command: "claude-agent-acp".into(),
            env: [("DISABLE_AUTOUPDATER".to_string(), "1".to_string())].into(),
            ..Default::default()
        };
        let active = Sandbox {
            name: "s".into(),
            ..Default::default()
        };
        assert_eq!(
            exec_args(&active, "", &harness(Some(image))),
            [
                "sandbox",
                "exec",
                "-n",
                "s",
                "--no-tty",
                "--timeout",
                "0",
                "--env",
                "DISABLE_AUTOUPDATER=1",
                "--",
                "claude-agent-acp"
            ]
            .map(String::from)
            .to_vec(),
            "the active gateway takes no -g"
        );
    }

    #[test]
    fn create_uploads_and_detaches() {
        let n = NewSandbox {
            gateway: "local".into(),
            name: "aleth-00ff".into(),
            from: "ghcr.io/me/claude-acp:1".into(),
            providers: vec!["anthropic".into(), " ".into()],
            upload: "/Users/me/src/app".into(),
            ..Default::default()
        };
        assert_eq!(
            create_args(&n),
            [
                "-g",
                "local",
                "sandbox",
                "create",
                "--name",
                "aleth-00ff",
                "--from",
                "ghcr.io/me/claude-acp:1",
                "--provider",
                "anthropic",
                "--upload",
                "/Users/me/src/app",
                "--detach",
                "--no-auto-providers",
                "--no-tty",
                "-o",
                "json"
            ]
            .map(String::from)
            .to_vec()
        );
    }

    #[test]
    fn a_terminal_line_is_quoted() {
        let sb = Sandbox {
            gateway: "my gw".into(),
            name: "s".into(),
            ..Default::default()
        };
        assert_eq!(
            terminal_command("openshell", &sb, "/sandbox/a b"),
            "openshell -g 'my gw' sandbox exec -n s --tty --workdir '/sandbox/a b' -- bash -l"
        );
    }

    #[test]
    fn names_fit_openshell() {
        let n = sandbox_name("acp-1A2B3C4D");
        assert_eq!(n, "aleth-1a2b3c4d");
        assert!(
            n.len() <= 19
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
    }

    #[test]
    fn cli_listings() {
        // Shapes from openshell-cli's gateway_to_json and sandbox_to_json.
        let g = parse_gateways(
            r#"[{"name":"local","endpoint":"http://127.0.0.1:17670","type":"local","source":"system","auth":"plaintext","active":true,"is_remote":false,"remote_host":null,"resolved_host":null},{"name":"k8s","endpoint":"https://gw.example.com","type":"remote","source":"user","auth":"edge","active":false,"is_remote":true}]"#,
        );
        assert_eq!(
            g,
            vec![
                json!({ "name": "local", "endpoint": "http://127.0.0.1:17670", "remote": false, "active": true }),
                json!({ "name": "k8s", "endpoint": "https://gw.example.com", "remote": true, "active": false })
            ]
        );
        let s = parse_sandboxes(
            r#"[{"id":"x","name":"aleth-1","phase":"Ready","labels":{}},{"id":"y","name":"old","phase":"Stopped"}]"#,
        );
        assert_eq!(
            s,
            vec![
                json!({ "name": "aleth-1", "phase": "Ready" }),
                json!({ "name": "old", "phase": "Stopped" })
            ]
        );
        assert!(parse_sandboxes("No sandboxes found.").is_empty());
    }
}

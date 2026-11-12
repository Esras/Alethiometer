//! Agents tab (design "Agents Final" 01–08, `app/docs/agents.md`): coding-agent
//! sessions in one tree.
//!
//! - Local ACP sessions (`acp`): Alethiometer is the ACP client and launches the
//!   agent itself. It only sees sessions it started. Local sessions can't
//!   take part in rooms, and nothing here builds a channel between them.
//! - OpenShell (`openshell`): a local ACP session whose adapter runs in an
//!   OpenShell sandbox, on a local or remote gateway.
//!
//! Local agents run in the agent host (`daemon`, `app/agentd`) so they outlive
//! the app. The UI polls `agents.poll` with the last revision it saw (`store`).

pub mod acp;
pub mod archive;
pub mod daemon;
pub mod explain;
pub mod facts;
pub mod incoming;
pub use crate::domain::agents::model;
pub mod openshell;
pub mod shell;
pub mod store;

use crate::api::arg_str;
use crate::error::{msg, Result};
use crate::state::App;
use model::{Group, Request, Row, RowImage, Session, Status};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default)]
pub struct Agents {
    pub store: store::Store,
    pub acp: acp::Host,
    pub daemon: daemon::Daemon,
    /// The last set of signals written, so an unchanged store writes nothing.
    pub incoming_print: std::sync::Mutex<String>,
    pub incoming_guess: std::sync::Mutex<Option<std::time::Instant>>,
}

/// Load local state, then keep it flushed and follow the agent host.
pub fn start(app: Arc<App>) {
    if let Err(e) = app.agents.store.load(app.store()) {
        eprintln!("agents: couldn't load sessions: {e}");
    }
    settle_projects(&app.agents.store);
    let a = app.clone();
    std::thread::Builder::new()
        .name("agents-flush".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if let Err(e) = a.agents.store.flush(a.store()) {
                eprintln!("agents: flush failed: {e}");
            }
        })
        .ok();
    // A live local session whose direnv files changed is flagged (the status
    // line offers a restart); stat'ed outside the store's lock.
    let a = app.clone();
    std::thread::Builder::new()
        .name("agents-env".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let watched: Vec<(String, Vec<model::EnvWatch>)> = a
                .agents
                .store
                .lock()
                .sessions
                .values()
                .filter(|s| s.live && !s.env_changed && !s.env_watches.is_empty())
                .map(|s| (s.id.clone(), s.env_watches.clone()))
                .collect();
            for (id, w) in watched {
                if acp::env_changed(&w) {
                    a.agents.store.lock().update_session(&id, |s| s.env_changed = true);
                }
            }
        })
        .ok();
    archive::refresh_failed(&app);
    daemon::start(app.clone());
    incoming::start(app.clone());
    openshell::start(app);
}

/// The app is quitting: write what's pending. Local agents keep running in
/// the agent host; the next start picks them up.
pub fn shutdown(app: &App) {
    let _ = app.agents.store.flush(app.store());
}

/// Now, as the store's rows hold times: a UTC instant (`crate::time::Ts`).
pub fn now(app: &App) -> String {
    app.clock.now().to_db()
}

/// The thread key a session's transcript lives under.
pub fn session_thread(id: &str) -> String {
    format!("s:{id}")
}

fn tree(app: &App) -> Value {
    let i = app.agents.store.lock();
    let mut groups: Vec<&Group> = i.groups.values().collect();
    groups.sort_by(|a, b| {
        (a.source != "acp", &a.project, &a.created_at).cmp(&(b.source != "acp", &b.project, &b.created_at))
    });
    let mut sessions: Vec<&Session> = i.sessions.values().collect();
    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut requests: Vec<&Request> = i.requests.values().collect();
    requests.sort_by(|a, b| a.at.cmp(&b.at));
    let count = |st: Status| sessions.iter().filter(|s| s.status == st).count();
    let now = app.clock.now();
    let nudges: serde_json::Map<String, Value> = groups
        .iter()
        .filter_map(|g| archive::nudge(&i, g, now).map(|t| (g.id.clone(), json!(t))))
        .collect();
    json!({
        "rev": i.tree_rev,
        "groups": groups,
        "sessions": sessions,
        "requests": requests,
        "counts": { "running": count(Status::Running), "waiting": count(Status::Waiting), "failed": count(Status::Failed) },
        "nudges": nudges,
        "deletes_failed": i.deletes_failed,
        "harnesses": app.settings().agents.harnesses.iter().map(|h| {
            let (cmd, args, _) = openshell::adapter(h);
            json!({
                "key": h.key,
                "label": h.label,
                "cache_ttl_min": h.cache_ttl_min,
                "command": format!("{} {}", h.command, h.args.join(" ")).trim().to_string(),
                "sandbox_command": format!("{cmd} {}", args.join(" ")).trim().to_string(),
                "sandbox_from": h.openshell.as_ref().map(|o| o.from.clone()).unwrap_or_default(),
            })
        }).collect::<Vec<_>>(),
        "openshell": openshell::cli(&app.settings().agents.openshell.command).is_some(),
        "agentd": daemon::status(app),
        "host": acp::host_label(),
    })
}

/// A thread's rows changed since `since`. `tab = "messages"` is screen 04:
/// you ↔ the agent, your mentions of it in rooms, and its asks.
fn thread(app: &App, key: &str, since: u64, tab: &str) -> Result<Value> {
    app.agents.store.ensure_loaded(app.store(), key)?;
    let i = app.agents.store.lock();
    let rows: Vec<Row> = match (tab, key.strip_prefix("s:")) {
        ("messages", Some(id)) => messages_view(&i, id),
        _ => i.threads.get(key).map(|t| t.rows.clone()).unwrap_or_default(),
    };
    // The messages view is computed, so it's always sent whole.
    let reset = since == 0 || tab == "messages";
    let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
    let changed: Vec<&Row> = rows.iter().filter(|r| reset || r.rev > since).collect();
    Ok(json!({ "key": key, "tab": tab, "rev": i.rev, "reset": reset, "rows": changed, "ids": ids }))
}

fn messages_view(i: &store::Inner, id: &str) -> Vec<Row> {
    let Some(s) = i.sessions.get(id) else { return vec![] };
    let own = i
        .threads
        .get(&session_thread(id))
        .map(|t| t.rows.as_slice())
        .unwrap_or_default();
    let mentions = |r: &Row| {
        r.text.contains(&format!("@{}", s.name)) || r.to.as_ref().is_some_and(|t| t.session.as_deref() == Some(id))
    };
    let mut out: Vec<Row> = own
        .iter()
        .filter(|r| {
            matches!(r.kind, model::Kind::You | model::Kind::Ask)
                || (r.kind == model::Kind::Message && r.to.as_ref().is_some_and(|t| t.kind == "you"))
        })
        .cloned()
        .collect();
    let room = s
        .group_id
        .as_ref()
        .and_then(|g| i.groups.get(g))
        .and_then(|g| g.room.as_ref())
        .map(|r| format!("r:{}", r.id));
    if let Some(room) = room.and_then(|k| i.threads.get(&k)) {
        let mut next = 1_000_000;
        // The agent's own thread may already hold the same message (same agent-side id).
        // Mention copies carry their own ids, so the time and text match too.
        let seen: std::collections::HashSet<String> = out
            .iter()
            .flat_map(|r| [r.ext.clone(), format!("{}|{}", r.at, r.text)])
            .filter(|k| !k.is_empty())
            .collect();
        let dup =
            |r: &Row| (!r.ext.is_empty() && seen.contains(&r.ext)) || seen.contains(&format!("{}|{}", r.at, r.text));
        for r in room
            .rows
            .iter()
            .filter(|r| r.who.as_ref().is_some_and(|w| w.kind == "you") && mentions(r) && !dup(r))
        {
            next += 1;
            out.push(Row { id: next, ..r.clone() });
        }
    }
    out.sort_by(|a, b| a.at.cmp(&b.at));
    out
}

fn opt<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn session_of(app: &App, id: &str) -> Result<Session> {
    app.agents
        .store
        .lock()
        .sessions
        .get(id)
        .cloned()
        .ok_or_else(|| msg(format!("no agent session '{id}'")))
}

/// The folders a local session works in, on this machine when it has them: what was uploaded into its
/// sandbox (the first, then the more directories), else its directory and its more directories.
pub(crate) fn local_folders(s: &Session) -> Vec<String> {
    match &s.sandbox {
        Some(b) if !b.upload.is_empty() => std::iter::once(b.upload.clone())
            .chain(b.extra.iter().map(|u| u.local.clone()))
            .collect(),
        _ => std::iter::once(s.cwd.clone()).chain(s.dirs.iter().cloned()).collect(),
    }
}

/// The set of repos `folders` are in (`facts::project_dir`), in their order, without repeats. Runs git.
pub(crate) fn repos_of<'a>(folders: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for f in folders.into_iter().filter(|f| !f.is_empty()) {
        let d = facts::project_dir(f);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// One key per set of repos, whatever their order: the tree heading, and the scope agent colours are unique in.
pub(crate) fn repo_set_key(repos: &[String]) -> String {
    let mut r: Vec<&str> = repos.iter().map(String::as_str).collect();
    r.sort_unstable();
    r.dedup();
    r.join("\n")
}

/// At startup: local groups are gone (user, 2026-11-12), so their sessions go to their repos; and sessions
/// stored before repo sets get theirs. Git runs outside the store's lock.
fn settle_projects(agents: &store::Store) {
    let folders: std::collections::BTreeSet<String> = agents
        .lock()
        .sessions
        .values()
        .filter(|s| s.source == "acp" && s.repos.is_empty() && !s.cwd.is_empty())
        .flat_map(local_folders)
        .filter(|f| !f.is_empty())
        .collect();
    let dirs = folders
        .into_iter()
        .map(|f| {
            let d = facts::project_dir(&f);
            (f, d)
        })
        .collect();
    assign_projects(&mut agents.lock(), &dirs);
}

/// Drop every local group (their sessions are then listed by repo set) and fill in each local session's
/// project and repos from `dirs` (folder to project folder).
fn assign_projects(i: &mut store::Inner, dirs: &std::collections::HashMap<String, String>) {
    let local: Vec<String> = i
        .groups
        .values()
        .filter(|g| g.source == "acp")
        .map(|g| g.id.clone())
        .collect();
    for id in &local {
        i.remove_group(id);
    }
    let ids: Vec<String> = i.sessions.keys().cloned().collect();
    for id in &ids {
        i.update_session(id, |s| {
            if s.source != "acp" || !s.repos.is_empty() {
                return;
            }
            let mut repos: Vec<String> = vec![];
            for f in local_folders(s) {
                if let Some(d) = dirs.get(&f) {
                    if !repos.contains(d) {
                        repos.push(d.clone());
                    }
                }
            }
            let Some(first) = repos.first() else { return };
            if s.project_dir.is_empty() {
                s.project = facts::project_name(first);
                s.project_dir = first.clone();
            }
            s.repos = repos;
        });
    }
}

/// What the Agents tab's badge counts: open requests, and local sessions that ended a turn you haven't looked at.
pub fn attention(app: &App) -> usize {
    let st = app.agents.store.lock();
    st.requests.len() + st.sessions.values().filter(|s| s.turn_unseen()).count()
}

/// `agents.<name>` commands.
pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        // One call per tick: the tree when it changed, and the open thread's new rows.
        "poll" => {
            let since = args.get("rev").and_then(Value::as_u64).unwrap_or(0);
            let tree_rev = app.agents.store.lock().tree_rev;
            let mut out = json!({ "rev": app.agents.store.rev(), "tree": null, "thread": null });
            if tree_rev > since || since == 0 {
                out["tree"] = tree(app);
            }
            if let Some(key) = opt(args, "thread") {
                let trev = args.get("thread_rev").and_then(Value::as_u64).unwrap_or(0);
                if trev == 0 || app.agents.store.rev() > trev {
                    out["thread"] = thread(app, key, trev, opt(args, "tab").unwrap_or("transcript"))?;
                }
            }
            Ok(out)
        }
        "tree" => Ok(tree(app)),
        "badge" => Ok(json!(attention(app))),
        "seen" => {
            let id = arg_str(args, "session")?;
            app.agents
                .store
                .lock()
                .update_session(&id, |s| s.seen_turns = Some(s.stats.turns));
            Ok(json!({ "ok": true }))
        }
        "thread" => thread(app, &arg_str(args, "key")?, 0, opt(args, "tab").unwrap_or("transcript")),
        // Message a session.
        "send" => {
            let key = arg_str(args, "thread")?;
            let text = arg_str(args, "text")?;
            let images: Vec<RowImage> = args
                .get("images")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            if text.trim().is_empty() && images.is_empty() {
                return Err(msg("Write a message first"));
            }
            let id = key
                .strip_prefix("s:")
                .ok_or_else(|| msg("Only a session takes messages"))?;
            acp::send(app, id, &text, images)?;
            Ok(json!({ "ok": true }))
        }
        "interrupt" => {
            acp::interrupt(app, &arg_str(args, "session")?)?;
            Ok(json!({ "ok": true }))
        }
        "send_queued" => {
            acp::send_queued(app, &arg_str(args, "session")?)?;
            Ok(json!({ "ok": true }))
        }
        // Approval: pick one of the agent's own options (O/S/A/D).
        "respond" => {
            let req = arg_str(args, "request")?;
            let option = arg_str(args, "option")?;
            if req.starts_with("osr:") {
                openshell::respond_rule(app, &req, &option, opt(args, "reason"))?;
            } else {
                acp::respond(app, &req, &option, opt(args, "reason"))?;
            }
            Ok(json!({ "ok": true }))
        }
        // Ask: a choice (1–3) or a written reply.
        "answer" => {
            acp::answer(
                app,
                &arg_str(args, "request")?,
                opt(args, "choice"),
                opt(args, "text"),
                args.get("content").filter(|c| c.is_object()).cloned(),
            )?;
            Ok(json!({ "ok": true }))
        }
        "start_local" => {
            let id = acp::start_session(
                app,
                &acp::StartArgs {
                    harness: arg_str(args, "harness")?,
                    cwd: arg_str(args, "cwd")?,
                    name: opt(args, "name").map(str::to_owned),
                    task: opt(args, "task").unwrap_or("").to_owned(),
                    model: opt(args, "model").map(str::to_owned),
                    thinking: opt(args, "thinking").map(str::to_owned),
                    dirs: args
                        .get("dirs")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                        .unwrap_or_default(),
                    branches: args
                        .get("branches")
                        .and_then(Value::as_object)
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                                .collect()
                        })
                        .unwrap_or_default(),
                    sandbox: args
                        .get("sandbox")
                        .filter(|v| v.is_object())
                        .map(|b| acp::SandboxStart {
                            gateway: opt(b, "gateway").unwrap_or("").to_owned(),
                            existing: opt(b, "existing").map(str::to_owned),
                            workdir: opt(b, "workdir").unwrap_or("").to_owned(),
                            from: opt(b, "from").unwrap_or("").to_owned(),
                        }),
                },
            )?;
            Ok(json!({ "id": id }))
        }
        "set_thinking" => {
            acp::set_thinking(app, &arg_str(args, "session")?, &arg_str(args, "value")?)?;
            Ok(json!({ "ok": true }))
        }
        "stop" => {
            acp::stop(app, &arg_str(args, "session")?);
            Ok(json!({ "ok": true }))
        }
        "remove" => {
            let id = arg_str(args, "session")?;
            acp::stop(app, &id);
            app.agents.store.lock().remove_session(&id);
            Ok(json!({ "ok": true }))
        }
        "archive_plan" => Ok(serde_json::to_value(archive::plan(
            &app.agents.store.lock(),
            &arg_str(args, "target")?,
        )?)?),
        "archive" => archive::archive(
            app,
            &arg_str(args, "target")?,
            args.get("force").and_then(Value::as_bool).unwrap_or(false),
        ),
        "nudge_snooze" => {
            archive::snooze(app, &arg_str(args, "group")?)?;
            Ok(json!({ "ok": true }))
        }
        "archived" => archive::search(app, args.get("q").and_then(Value::as_str).unwrap_or("")),
        "archived_open" => archive::open(app, &arg_str(args, "id")?),
        "project_of" => {
            let dir = facts::project_dir(&acp::expand_home(&arg_str(args, "cwd")?));
            Ok(json!({ "project": facts::project_name(&dir), "project_dir": dir }))
        }
        // The New agent form's Directories: whether each is there, and the repo it's in (for the top line).
        "dir_info" => {
            let dirs: Vec<String> = args
                .get("dirs")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            Ok(json!(dirs
                .iter()
                .map(|d| {
                    let path = acp::expand_home(d.trim());
                    let exists = std::path::Path::new(&path).is_dir();
                    let repo = if exists {
                        facts::project_dir(&path)
                    } else {
                        String::new()
                    };
                    json!({
                        "dir": d,
                        "exists": exists,
                        "name": openshell::dir_name(&path),
                        "project": if exists { facts::project_name(&repo) } else { String::new() },
                        "project_dir": repo,
                    })
                })
                .collect::<Vec<_>>()))
        }
        // Replace the saved agents (the New agent form's list) in settings.json.
        "saved" => Ok(json!(app.settings().agents.saved)),
        "saved_set" => {
            let saved = args
                .get("saved")
                .filter(|v| v.is_array())
                .cloned()
                .ok_or_else(|| msg("saved: a list"))?;
            let list: Vec<crate::settings::SavedAgent> = serde_json::from_value(saved.clone())?;
            if let Some(a) = list.iter().find(|a| a.name.trim().is_empty()) {
                return Err(msg(format!("A saved agent needs a name ({} has none)", a.harness)));
            }
            crate::settings::live::patch(
                app,
                &[crate::settings::live::Edit::Set(
                    vec!["agents".into(), "saved".into()],
                    saved,
                )],
            )?;
            Ok(json!({ "ok": true }))
        }
        "explain" => {
            explain::request(app, &arg_str(args, "request")?);
            Ok(json!({ "ok": true }))
        }
        "facts" => {
            facts::refresh(app, &arg_str(args, "session")?);
            Ok(json!({ "ok": true }))
        }
        // Open in Zed, or a terminal in the session's directory (or its sandbox).
        "open" => {
            let s = session_of(app, &arg_str(args, "session")?)?;
            if let Some(sb) = &s.sandbox {
                let line = openshell::terminal_command(&app.settings().agents.openshell.command, sb, &s.cwd);
                app.platform
                    .open_terminal("", Some(&line))
                    .map_err(|e| msg(e.to_string()))?;
            } else if opt(args, "how") == Some("terminal") {
                app.platform
                    .open_terminal(&s.cwd, None)
                    .map_err(|e| msg(e.to_string()))?;
            } else {
                acp::open_in_editor(app, &s.cwd)?;
            }
            Ok(json!({ "ok": true }))
        }
        // Where an OpenShell agent can run: the CLI's gateways, and a gateway's sandboxes.
        "openshell_gateways" => openshell::gateways(&app.settings().agents.openshell.command),
        // Bring an agent's changes back out of its sandbox: look (downloads), then copy (or drop the look).
        "sandbox_changes" => openshell::preview(
            app,
            &arg_str(args, "session")?,
            args.get("folder").and_then(Value::as_u64).unwrap_or(0) as usize,
        ),
        "sandbox_copy_back" => openshell::copy_back(
            app,
            &arg_str(args, "session")?,
            args.get("force").and_then(Value::as_bool).unwrap_or(false),
        ),
        "sandbox_discard" => {
            openshell::sync::discard(&arg_str(args, "session")?);
            Ok(json!({ "ok": true }))
        }
        "openshell_sandboxes" => openshell::sandboxes(
            &app.settings().agents.openshell.command,
            opt(args, "gateway").unwrap_or(""),
        ),
        "models" => acp::models(app, &arg_str(args, "harness")?, opt(args, "cwd").unwrap_or("")),
        "dirs" => {
            let r = acp::recent_dirs(app);
            Ok(json!({
                "dirs": r.iter().map(|(d, _)| d).collect::<Vec<_>>(),
                "recent": r.iter().map(|(d, at)| json!({ "dir": d, "at": at })).collect::<Vec<_>>(),
            }))
        }
        // Stop every local agent (Quit no longer does).
        "stop_all" => Ok(json!({ "stopped": acp::stop_all(app) })),
        // Restart the agent host on the newest build (stops every local agent).
        "host_restart" => {
            daemon::restart(app)?;
            daemon::kick(app);
            Ok(json!({ "ok": true }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

#[cfg(test)]
mod project_tests {
    use super::model::{Group, Sandbox, Session, Upload};

    #[test]
    fn local_groups_are_dropped_and_their_sessions_listed_by_repo_set() {
        let mut i = super::store::Inner::default();
        for (id, source) in [("local:1", "acp"), ("miao:c1", "miao")] {
            i.put_group(Group {
                id: id.into(),
                source: source.into(),
                ..Default::default()
            });
        }
        let s = |id: &str, cwd: &str, dirs: &[&str]| Session {
            id: id.into(),
            source: "acp".into(),
            group_id: Some("local:1".into()),
            cwd: cwd.into(),
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        };
        i.put_session(s("acp-1", "/r/aleth/.claude/worktrees/jolly", &[]));
        i.put_session(s("acp-2", "/r/aleth/app", &["/r/pi-acp", "/r/aleth/core"]));
        i.put_session(Session {
            sandbox: Some(Sandbox {
                upload: "/r/k8s".into(),
                extra: vec![Upload {
                    local: "/r/aleth".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..s("acp-3", "/sandbox/k8s", &[])
        });
        let dirs = [
            ("/r/aleth/.claude/worktrees/jolly", "/r/aleth"),
            ("/r/aleth/app", "/r/aleth"),
            ("/r/aleth/core", "/r/aleth"),
            ("/r/aleth", "/r/aleth"),
            ("/r/pi-acp", "/r/pi-acp"),
            ("/r/k8s", "/r/k8s"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
        super::assign_projects(&mut i, &dirs);
        assert!(!i.groups.contains_key("local:1"));
        assert!(i.groups.contains_key("miao:c1"), "a remote group stays");
        assert!(i.sessions.values().all(|s| s.group_id.is_none()));
        assert_eq!(i.sessions["acp-1"].repos, ["/r/aleth"]);
        assert_eq!(i.sessions["acp-2"].repos, ["/r/aleth", "/r/pi-acp"]);
        assert_eq!(i.sessions["acp-2"].project, "aleth");
        assert_eq!(i.sessions["acp-3"].repos, ["/r/k8s", "/r/aleth"]);
        assert_eq!(i.sessions["acp-3"].project_dir, "/r/k8s");
    }

    #[test]
    fn a_repo_set_is_the_same_set_in_any_order() {
        let k = |r: &[&str]| super::repo_set_key(&r.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(k(&["/r/b", "/r/a"]), k(&["/r/a", "/r/b"]));
        assert_ne!(k(&["/r/a"]), k(&["/r/a", "/r/b"]));
    }
}

#[cfg(test)]
mod saved_tests {
    use serde_json::json;

    #[test]
    fn saved_agents_are_kept_in_settings_json_in_their_order() {
        let (app, dir) = crate::ai::context::tests::app();
        std::fs::write(dir.path().join("settings.json"), "{}").unwrap();
        let list = json!([
            { "name": "Sandbox Pi", "runs": "openshell", "harness": "pi", "dirs": ["~/src/a", "~/src/b"] },
            { "name": "Reviewer", "runs": "local", "harness": "claude", "task": "Review the diff." },
        ]);
        super::call(&app, "saved_set", &json!({ "saved": list })).unwrap();
        let back = super::call(&app, "saved", &json!({})).unwrap();
        assert_eq!(back[0]["name"], "Sandbox Pi");
        assert_eq!(back[0]["dirs"], json!(["~/src/a", "~/src/b"]));
        assert_eq!(back[1]["task"], "Review the diff.");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(file["agents"]["saved"][1]["name"], "Reviewer");
        let unnamed = json!({ "saved": [{ "name": " ", "harness": "pi" }] });
        assert!(super::call(&app, "saved_set", &unnamed).is_err());
    }
}

//! Agents tab (design "Agents Final" 01–08, `app/docs/agents.md`): coding-agent
//! sessions from two kinds of source in one tree.
//!
//! - Local ACP sessions (`acp`): Alethiometer is the ACP client and launches the
//!   agent itself. It only sees sessions it started. Local sessions can't
//!   take part in rooms, and nothing here builds a channel between them.
//! - Scion agents (`scion`): Alethiometer is a Hub client. Rooms, DMs and
//!   delivery states all come from the Hub; there's no messaging layer here.
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
pub mod guess;
pub mod incoming;
pub mod model;
pub mod openshell;
pub mod scion;
pub mod shell;
pub mod store;

use crate::error::{msg, Result};
use crate::queries::arg_str;
use crate::state::App;
use model::{Group, Request, Row, RowImage, Session, Status};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default)]
pub struct Agents {
    pub store: store::Store,
    pub acp: acp::Host,
    pub daemon: daemon::Daemon,
    pub scion: scion::Hubs,
    /// The last set of signals written, so an unchanged store writes nothing.
    pub incoming_print: std::sync::Mutex<String>,
    pub incoming_guess: std::sync::Mutex<Option<std::time::Instant>>,
}

/// Load local state, then keep it flushed, follow the agent host, and keep
/// the Scion Hubs connected.
pub fn start(app: Arc<App>) {
    if let Err(e) = app.agents.store.load(app.store()) {
        eprintln!("agents: couldn't load sessions: {e}");
    }
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
            let watched: Vec<(String, Vec<model::EnvWatch>)> =
                a.agents.store.lock().sessions.values().filter(|s| s.live && !s.env_changed && !s.env_watches.is_empty()).map(|s| (s.id.clone(), s.env_watches.clone())).collect();
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
    scion::start(app);
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
    groups.sort_by(|a, b| (a.source != "acp", &a.project, &a.created_at).cmp(&(b.source != "acp", &b.project, &b.created_at)));
    let mut sessions: Vec<&Session> = i.sessions.values().collect();
    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut requests: Vec<&Request> = i.requests.values().collect();
    requests.sort_by(|a, b| a.at.cmp(&b.at));
    let count = |st: Status| sessions.iter().filter(|s| s.status == st).count();
    let now = app.clock.now();
    let nudges: serde_json::Map<String, Value> = groups.iter().filter_map(|g| archive::nudge(&i, g, now).map(|t| (g.id.clone(), json!(t)))).collect();
    json!({
        "rev": i.tree_rev,
        "groups": groups,
        "sessions": sessions,
        "requests": requests,
        "counts": { "running": count(Status::Running), "waiting": count(Status::Waiting), "failed": count(Status::Failed) },
        "nudges": nudges,
        "hub_failed": i.hub_failed,
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
        "hubs": scion::hubs(app),
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
    let own = i.threads.get(&session_thread(id)).map(|t| t.rows.as_slice()).unwrap_or_default();
    let mentions = |r: &Row| r.text.contains(&format!("@{}", s.name)) || r.to.as_ref().is_some_and(|t| t.session.as_deref() == Some(id));
    let mut out: Vec<Row> = own
        .iter()
        .filter(|r| matches!(r.kind, model::Kind::You | model::Kind::Ask) || (r.kind == model::Kind::Message && r.to.as_ref().is_some_and(|t| t.kind == "you")))
        .cloned()
        .collect();
    let room = s.group_id.as_ref().and_then(|g| i.groups.get(g)).and_then(|g| g.room.as_ref()).map(|r| format!("r:{}", r.id));
    if let Some(room) = room.and_then(|k| i.threads.get(&k)) {
        let mut next = 1_000_000;
        // The agent's own thread may already hold the same message (same Hub id).
        // Mention copies carry their own ids, so the time and text match too.
        let seen: std::collections::HashSet<String> = out.iter().flat_map(|r| [r.ext.clone(), format!("{}|{}", r.at, r.text)]).filter(|k| !k.is_empty()).collect();
        let dup = |r: &Row| (!r.ext.is_empty() && seen.contains(&r.ext)) || seen.contains(&format!("{}|{}", r.at, r.text));
        for r in room.rows.iter().filter(|r| r.who.as_ref().is_some_and(|w| w.kind == "you") && mentions(r) && !dup(r)) {
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
    app.agents.store.lock().sessions.get(id).cloned().ok_or_else(|| msg(format!("no agent session '{id}'")))
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
            app.agents.store.lock().update_session(&id, |s| s.seen_turns = Some(s.stats.turns));
            Ok(json!({ "ok": true }))
        }
        "thread" => thread(app, &arg_str(args, "key")?, 0, opt(args, "tab").unwrap_or("transcript")),
        // Message a session or a room. `to` addresses one room member (@).
        "send" => {
            let key = arg_str(args, "thread")?;
            let text = arg_str(args, "text")?;
            let images: Vec<RowImage> = args
                .get("images")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
                .unwrap_or_default();
            if text.trim().is_empty() && images.is_empty() {
                return Err(msg("Write a message first"));
            }
            match key.strip_prefix("s:") {
                Some(id) if id.starts_with("acp-") => acp::send(app, id, &text, images)?,
                _ => {
                    if !images.is_empty() {
                        return Err(msg("Images aren't supported on Scion yet"));
                    }
                    scion::send(app, &key, &text, opt(args, "to"))?
                }
            }
            Ok(json!({ "ok": true }))
        }
        "interrupt" => {
            let id = arg_str(args, "session")?;
            if id.starts_with("acp-") {
                acp::interrupt(app, &id)?
            } else {
                scion::interrupt(app, &id)?
            }
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
            acp::respond(app, &req, &option, opt(args, "reason"))?;
            Ok(json!({ "ok": true }))
        }
        // Ask: a choice (1–3) or a written reply.
        "answer" => {
            let req = arg_str(args, "request")?;
            if req.starts_with("acp-") {
                acp::answer(app, &req, opt(args, "choice"), opt(args, "text"), args.get("content").filter(|c| c.is_object()).cloned())?;
            } else {
                scion::answer(app, &req, &arg_str(args, "text")?)?;
            }
            Ok(json!({ "ok": true }))
        }
        "start_local" => {
            let id = acp::start_session(
                app,
                &acp::StartArgs {
                    harness: arg_str(args, "harness")?,
                    cwd: arg_str(args, "cwd")?,
                    name: opt(args, "name").map(str::to_owned),
                    group_id: opt(args, "group").map(str::to_owned),
                    task: opt(args, "task").unwrap_or("").to_owned(),
                    model: opt(args, "model").map(str::to_owned),
                    thinking: opt(args, "thinking").map(str::to_owned),
                    sandbox: args.get("sandbox").filter(|v| v.is_object()).map(|b| acp::SandboxStart {
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
        "start_scion" => Ok(json!({ "id": scion::start_agent(app, args)? })),
        "catalog" => scion::catalog(app, &arg_str(args, "group")?),
        "stop" => {
            let id = arg_str(args, "session")?;
            if id.starts_with("acp-") {
                acp::stop(app, &id);
            } else {
                scion::stop(app, &id)?;
            }
            Ok(json!({ "ok": true }))
        }
        "remove" => {
            let id = arg_str(args, "session")?;
            if !id.starts_with("acp-") {
                return Err(msg("Scion agents are removed on the Hub"));
            }
            acp::stop(app, &id);
            app.agents.store.lock().remove_session(&id);
            Ok(json!({ "ok": true }))
        }
        "archive_plan" => Ok(serde_json::to_value(archive::plan(&app.agents.store.lock(), &arg_str(args, "target")?)?)?),
        "archive" => archive::archive(app, &arg_str(args, "target")?, args.get("force").and_then(Value::as_bool).unwrap_or(false)),
        "nudge_snooze" => {
            archive::snooze(app, &arg_str(args, "group")?)?;
            Ok(json!({ "ok": true }))
        }
        "archived" => archive::search(app, args.get("q").and_then(Value::as_str).unwrap_or("")),
        "archived_open" => archive::open(app, &arg_str(args, "id")?),
        "group_create" => {
            let name = arg_str(args, "name")?;
            let id = format!("local:{}", next_local_id(app));
            let project = opt(args, "project").map(str::to_owned).unwrap_or_else(|| {
                opt(args, "cwd").map(facts::project_of).unwrap_or_default()
            });
            app.agents.store.lock().put_group(Group {
                id: id.clone(),
                source: "acp".into(),
                project,
                name: name.trim().into(),
                goal: opt(args, "goal").unwrap_or("").trim().into(),
                host: acp::host_label(),
                room: None,
                created_at: now(app),
            });
            Ok(json!({ "id": id }))
        }
        "group_update" => {
            let id = arg_str(args, "group")?;
            let mut i = app.agents.store.lock();
            let mut g = i.groups.get(&id).cloned().ok_or_else(|| msg("no such group"))?;
            if g.source != "acp" {
                return Err(msg("Scion groups are edited on the Hub"));
            }
            if let Some(n) = opt(args, "name") {
                g.name = n.trim().into();
            }
            if let Some(goal) = args.get("goal").and_then(Value::as_str) {
                g.goal = goal.trim().into();
            }
            i.put_group(g);
            Ok(json!({ "ok": true }))
        }
        "group_delete" => {
            let id = arg_str(args, "group")?;
            if !id.starts_with("local:") {
                return Err(msg("Scion groups are removed on the Hub"));
            }
            app.agents.store.lock().remove_group(&id);
            Ok(json!({ "ok": true }))
        }
        // File a local session under a group (or back to Unsorted).
        "move" => {
            let id = arg_str(args, "session")?;
            let group = opt(args, "group").map(str::to_owned);
            if !id.starts_with("acp-") {
                return Err(msg("Scion agents belong to their Hub group"));
            }
            let color = {
                let s = session_of(app, &id)?;
                let palette = app.settings().agents.names.palette.len();
                store::color_for(app.store(), group.as_deref().unwrap_or("unsorted"), &s.id, palette)?
            };
            app.agents.store.lock().update_session(&id, |s| {
                s.group_id = group;
                s.guess = None;
                s.color = color;
            });
            Ok(json!({ "ok": true }))
        }
        "guess_dismiss" => {
            let id = arg_str(args, "session")?;
            app.agents.store.lock().update_session(&id, |s| {
                if let Some(g) = &mut s.guess {
                    g.group_id.clear();
                }
            });
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
        // Retry, forward or discard an undelivered message (08).
        "delivery" => scion::delivery(app, &arg_str(args, "thread")?, args.get("row").and_then(Value::as_u64).unwrap_or(0), &arg_str(args, "action")?, opt(args, "to")),
        // Open in Zed / on the Hub / a terminal attached to the session.
        "open" => {
            let id = arg_str(args, "session")?;
            let how = opt(args, "how").unwrap_or("app");
            let s = session_of(app, &id).or_else(|_| {
                // A room: Open on Hub.
                let i = app.agents.store.lock();
                i.groups
                    .values()
                    .find_map(|g| g.room.as_ref().filter(|r| format!("r:{}", r.id) == id).map(|r| Session { url: r.url.clone(), source: "scion".into(), ..Default::default() }))
                    .ok_or_else(|| msg("nothing to open"))
            })?;
            if let Some(sb) = s.sandbox.as_ref().filter(|_| s.source == "acp") {
                // No editor reaches into a sandbox: both open a shell in it.
                let line = openshell::terminal_command(&app.settings().agents.openshell.command, sb, &s.cwd);
                app.platform.open_terminal("", Some(&line)).map_err(|e| msg(e.to_string()))?;
                return Ok(json!({ "ok": true }));
            }
            match (s.source.as_str(), how) {
                ("acp", "terminal") => app.platform.open_terminal(&s.cwd, None).map_err(|e| msg(e.to_string()))?,
                ("acp", _) => acp::open_in_editor(app, &s.cwd)?,
                (_, "terminal") => app.platform.open_terminal("", Some(&scion::attach_command(&s))).map_err(|e| msg(e.to_string()))?,
                _ => app.platform.open_url(&s.url).map_err(|e| msg(e.to_string()))?,
            }
            Ok(json!({ "ok": true }))
        }
        // Where an OpenShell agent can run: the CLI's gateways, and a gateway's sandboxes.
        "openshell_gateways" => openshell::gateways(&app.settings().agents.openshell.command),
        "openshell_sandboxes" => openshell::sandboxes(&app.settings().agents.openshell.command, opt(args, "gateway").unwrap_or("")),
        "models" => acp::models(app, &arg_str(args, "harness")?, opt(args, "cwd").unwrap_or("")),
        "dirs" => Ok(json!({ "dirs": acp::recent_dirs(app) })),
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

fn next_local_id(app: &App) -> u64 {
    let i = app.agents.store.lock();
    i.groups.keys().filter_map(|k| k.strip_prefix("local:")?.parse::<u64>().ok()).max().unwrap_or(0) + 1
}

//! Agents in Incoming (design "Agents Incoming" 2a–2c, app/docs/agents.md).
//!
//! What an agent needs from you, or has for you, becomes a signal of a
//! built-in `agents` source, so it's filed under topics, confirmed, moved and
//! dismissed like any other. Waiting on you: an approval or ask, a message
//! that wasn't delivered, a session that failed. Not waiting: a Scion message
//! to you, the end of each local turn, a Scion agent that finished.
//! Subagents never show up by themselves.
//!
//! The store is the truth: `sync` turns it into items and marks the ones that
//! no longer apply as seen (`$.auto_seen`), so acting in Agents takes them out
//! of Incoming. A signal resolved that way comes back if its cause does (an
//! approval rebuilt after a restart); one you dismissed stays dismissed.

use super::model::{Kind, Row, Session, Status};
use super::store::Inner;
use crate::domain::ports::UnitOfWork;
use crate::error::Result;
use crate::state::App;
use crate::sync::{self, Batch, ItemUpsert, Message};
use crate::time::Ts;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const KIND: &str = "agents";

/// One signal the store says should be in Incoming.
#[derive(Debug, Clone, PartialEq)]
pub struct Sig {
    pub ext: String,
    pub session: String,
    /// approval | ask | undelivered | failed_delivery | failed | wrote | turn | done
    pub kind: String,
    /// Who it's from: the agent's name, or `#room`.
    pub name: String,
    /// "needs approval", "has a question", …
    pub reason: String,
    pub waiting: bool,
    pub at: String,
    pub ended: Option<String>,
    /// The conversation it's about: (at, who, text, faded).
    pub messages: Vec<(String, String, String, bool)>,
    /// The line under the messages on why it's here.
    pub why: String,
    pub info: Value,
    /// For the topic guess: group, goal, first task, directory.
    pub context: String,
    pub ticket: Option<String>,
}

/// Everything `sync` would write, and the sessions it can't judge yet (their
/// transcript isn't loaded): their signals are left as they are.
pub fn signals(i: &Inner) -> (Vec<Sig>, HashSet<String>) {
    let mut out = vec![];
    let mut unknown = HashSet::new();
    let top = |id: &str| -> Option<&Session> {
        let s = i.sessions.get(id)?;
        match &s.parent_id {
            Some(p) => i.sessions.get(p),
            None => Some(s),
        }
    };
    let base = |s: &Session| -> (Value, String, Option<String>) {
        let g = s.group_id.as_ref().and_then(|g| i.groups.get(g));
        let task = i
            .threads
            .get(&format!("s:{}", s.id))
            .and_then(|t| t.rows.iter().find(|r| r.kind == Kind::You))
            .map(|r| r.text.clone())
            .unwrap_or_default();
        let info = json!({
            "session": s.id,
            "project": g.map(|g| g.project.clone()).unwrap_or_default(),
            "group": g.map(|g| g.name.clone()).unwrap_or_default(),
            "harness": s.harness,
            "host": s.host,
            "cwd": super::acp::tilde(&s.cwd),
        });
        let context = match g {
            Some(g) => format!("Group: {} — {}\nFirst task: {}", g.name, g.goal, clip(&task, 400)),
            None => format!(
                "Directory: {}\nFirst task: {}",
                super::acp::tilde(&s.cwd),
                clip(&task, 400)
            ),
        };
        (
            info,
            context,
            s.facts
                .ticket
                .as_ref()
                .map(|t| t.label.clone())
                .filter(|t| !t.is_empty()),
        )
    };
    let sig = |s: &Session, ext: String, kind: &str, reason: String, waiting: bool, at: String| -> Sig {
        let (mut info, context, ticket) = base(s);
        info["kind"] = json!(kind);
        info["name"] = json!(s.name);
        info["reason"] = json!(reason);
        info["waiting"] = json!(waiting);
        Sig {
            ext,
            session: s.id.clone(),
            kind: kind.into(),
            name: s.name.clone(),
            reason,
            waiting,
            at,
            ended: None,
            messages: vec![],
            why: String::new(),
            info,
            context,
            ticket,
        }
    };
    let you_before = |rows: &[Row], before: usize| {
        rows[..before]
            .iter()
            .rev()
            .find(|r| r.kind == Kind::You)
            .map(|r| (r.at.clone(), "You".to_owned(), r.text.clone(), true))
    };

    // Approvals and asks.
    for r in i.requests.values() {
        let Some(s) = top(&r.session_id) else { continue };
        let ask = r.kind == "ask";
        // A prompt an extension raised (`tag: question`) is answered with the agent's options like an approval, but it's a question.
        let reason = if ask || r.tag == "question" {
            "has a question"
        } else {
            "needs approval"
        };
        let mut x = sig(
            s,
            format!("req:{}", r.id),
            if ask { "ask" } else { "approval" },
            reason.into(),
            true,
            r.at.clone(),
        );
        x.info["request"] = json!(r.id);
        if let Some(t) = i.threads.get(&r.thread) {
            let at = t
                .rows
                .iter()
                .position(|row| row.request.as_deref() == Some(r.id.as_str()))
                .unwrap_or(t.rows.len());
            if let Some(m) = you_before(&t.rows, at) {
                x.messages.push(m);
            }
        }
        if ask {
            x.messages
                .push((r.at.clone(), s.name.clone(), r.question.clone(), false));
            x.why = if r.thread.starts_with("r:") {
                format!("Asked in {}", room_name(i, &r.thread))
            } else {
                "Asked in its session".into()
            };
        } else if r.tag == "question" {
            x.why = "Asked in its session".into();
        } else if r.id.starts_with("osr:") {
            x.why = format!(
                "Its sandbox blocked a connection; OpenShell drafted this rule ({})",
                r.tool
            );
        } else {
            x.why = format!("Wants to run this in {}", super::acp::tilde(&r.cwd));
        }
        out.push(x);
    }

    for s in i.sessions.values().filter(|s| s.parent_id.is_none()) {
        let key = format!("s:{}", s.id);
        let thread = i.threads.get(&key);
        if s.source == "acp" && !thread.is_some_and(|t| t.loaded) {
            unknown.insert(s.id.clone());
            continue;
        }
        let rows: &[Row] = thread.map(|t| t.rows.as_slice()).unwrap_or(&[]);
        let last_you = rows.iter().rposition(|r| r.kind == Kind::You);
        match s.status {
            Status::Failed => {
                let reason = if s.problem.is_empty() {
                    "stopped".to_owned()
                } else {
                    format!("stopped: {}", clip(&s.problem, 60))
                };
                let mut x = sig(
                    s,
                    format!("failed:{}:{}", s.id, s.active_at),
                    "failed",
                    reason,
                    true,
                    s.active_at.clone(),
                );
                if let Some(r) = rows.iter().rev().find(|r| {
                    matches!(r.kind, Kind::Output | Kind::Message) && r.who.as_ref().is_none_or(|w| w.kind != "you")
                }) {
                    x.messages
                        .push((r.at.clone(), s.name.clone(), clip(&r.text, 1200), false));
                }
                x.why = [s.problem.as_str(), s.harness.as_str(), s.host.as_str()]
                    .iter()
                    .filter(|p| !p.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ");
                out.push(x);
            }
            Status::Done if s.source == "scion" => {
                let mut x = sig(
                    s,
                    format!("done:{}", s.id),
                    "done",
                    "finished".into(),
                    false,
                    s.active_at.clone(),
                );
                if let Some(r) = rows.iter().rev().find(|r| {
                    matches!(r.kind, Kind::Output | Kind::Message) && r.who.as_ref().is_none_or(|w| w.kind != "you")
                }) {
                    x.messages
                        .push((r.at.clone(), s.name.clone(), clip(&r.text, 1200), false));
                }
                let task = rows
                    .iter()
                    .find(|r| r.kind == Kind::You)
                    .map(|r| r.text.clone())
                    .unwrap_or_default();
                x.info["facts"] = json!({
                    "task": clip(&task, 120),
                    "ran": ran(&s.stats.started_at, &s.active_at),
                    "turns": s.stats.turns,
                    "pr": s.facts.pr.as_ref().map(|f| json!({ "label": f.label, "detail": f.detail, "url": f.url })),
                    "ticket": s.facts.ticket.as_ref().map(|f| f.label.clone()),
                });
                out.push(x);
            }
            // A local turn that ended with output after your last message: one signal per turn.
            Status::Idle if s.source == "acp" && s.stats.turns > 0 => {
                let out_at = rows.iter().rposition(|r| r.kind == Kind::Output);
                if let Some(o) = out_at.filter(|o| last_you.is_none_or(|y| *o > y)) {
                    let mut x = sig(
                        s,
                        format!("turn:{}:{}", s.id, s.stats.turns),
                        "turn",
                        "ended its turn".into(),
                        false,
                        rows[o].at.clone(),
                    );
                    if let Some(m) = you_before(rows, o) {
                        x.messages.push(m);
                    }
                    x.messages
                        .push((rows[o].at.clone(), s.name.clone(), clip(&rows[o].text, 2000), false));
                    out.push(x);
                }
            }
            _ => {}
        }
    }

    // Scion: messages to you since your last word, and messages the Hub didn't deliver.
    for (key, t) in &i.threads {
        let room = key.starts_with("r:");
        if !room && !key.starts_with("s:scion:") {
            continue;
        }
        let last_you = t.rows.iter().rposition(|r| r.kind == Kind::You);
        let mut wrote: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
        for (n, r) in t.rows.iter().enumerate() {
            if r.kind == Kind::Message
                && r.to.as_ref().is_some_and(|w| w.kind == "you")
                && last_you.is_none_or(|y| n > y)
            {
                if let Some(sid) = r.who.as_ref().and_then(|w| w.session.clone()) {
                    wrote.entry(sid).or_default().push(r);
                }
            }
            // Your message, or the failed-delivery row it turned into.
            let Some(d) = r.delivery.as_ref().filter(|d| {
                matches!(r.kind, Kind::You | Kind::Failed) && matches!(d.state.as_str(), "unreachable" | "failed")
            }) else {
                continue;
            };
            let to =
                r.to.as_ref()
                    .and_then(|w| w.session.clone())
                    .or_else(|| key.strip_prefix("s:").map(str::to_owned));
            let Some(s) = to.as_deref().and_then(top).or_else(|| first_member(i, key)) else {
                continue;
            };
            let failed = d.state == "failed";
            let mut x = sig(
                s,
                format!(
                    "undelivered:{key}:{}",
                    if r.ext.is_empty() {
                        r.id.to_string()
                    } else {
                        r.ext.clone()
                    }
                ),
                if failed { "failed_delivery" } else { "undelivered" },
                if failed {
                    "didn’t take your message".into()
                } else {
                    "didn’t get your message".into()
                },
                true,
                r.at.clone(),
            );
            if room && failed {
                x.name = room_name(i, key);
                x.info["name"] = json!(x.name);
            }
            x.info["thread"] = json!(key);
            x.info["row"] = json!(r.id);
            x.messages.push((r.at.clone(), "You".into(), r.text.clone(), false));
            x.why = if failed {
                format!(
                    "Failed. {}{}",
                    if d.reason.is_empty() {
                        "The Hub refused it."
                    } else {
                        d.reason.as_str()
                    },
                    if d.retry_at.is_empty() {
                        " It gives no retry time."
                    } else {
                        ""
                    }
                )
            } else {
                format!(
                    "Not delivered. {}",
                    if d.reason.is_empty() {
                        "Nothing is reading its messages.".to_owned()
                    } else {
                        d.reason.clone()
                    }
                )
            };
            out.push(x);
        }
        for (sid, msgs) in wrote {
            let Some(s) = top(&sid) else { continue };
            let first = msgs[0];
            let mut x = sig(
                s,
                format!(
                    "wrote:{key}:{}",
                    if first.ext.is_empty() {
                        first.id.to_string()
                    } else {
                        first.ext.clone()
                    }
                ),
                "wrote",
                "wrote to you".into(),
                false,
                first.at.clone(),
            );
            if msgs.len() > 1 {
                x.ended = msgs.last().map(|r| r.at.clone());
            }
            if let Some(m) = last_you.map(|y| &t.rows[y]) {
                x.messages.push((m.at.clone(), "You".into(), m.text.clone(), true));
            }
            for r in &msgs {
                x.messages.push((r.at.clone(), s.name.clone(), r.text.clone(), false));
            }
            x.info["thread"] = json!(key);
            x.info["count"] = json!(msgs.len());
            out.push(x);
        }
    }
    (out, unknown)
}

fn room_name(i: &Inner, key: &str) -> String {
    let id = key.trim_start_matches("r:");
    i.groups
        .values()
        .find_map(|g| g.room.as_ref().filter(|r| r.id == id).map(|r| format!("#{}", r.name)))
        .unwrap_or_else(|| "#room".into())
}

/// A room's message with no single recipient: credit the room's group's first member.
fn first_member<'a>(i: &'a Inner, key: &str) -> Option<&'a Session> {
    let id = key.trim_start_matches("r:");
    let g = i
        .groups
        .values()
        .find(|g| g.room.as_ref().is_some_and(|r| r.id == id))?;
    i.sessions
        .values()
        .find(|s| s.group_id.as_deref() == Some(g.id.as_str()) && s.parent_id.is_none())
}

fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        return t.to_owned();
    }
    format!("{}…", t.chars().take(n - 1).collect::<String>().trim_end())
}

fn ran(from: &str, to: &str) -> String {
    let (Some(a), Some(b)) = (Ts::parse(from), Ts::parse(to)) else {
        return String::new();
    };
    let m = (b - a).as_mins().max(0);
    if m >= 60 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// The `agents` source's row, made the first time.
pub fn source_id(uow: &dyn UnitOfWork) -> Result<i64> {
    uow.sources().ensure_local(KIND, "Agents")
}

/// Bring Incoming in line with the store. Returns whether anything changed.
pub fn sync(app: &App) -> Result<bool> {
    let (sigs, unknown) = signals(&app.agents.store.lock());
    let print = format!("{sigs:?}|{unknown:?}");
    {
        let mut last = app.agents.incoming_print.lock().unwrap_or_else(|e| e.into_inner());
        if *last == print {
            return Ok(false);
        }
        *last = print;
    }
    // One unit of work: the signals, what no longer applies, and their topics.
    let needs_guess = app.store().write(|uow| {
        let sid = source_id(uow)?;
        let source = sync::source(uow.reads(), sid)?;
        // What's there: external id → (seen, resolved by us, session).
        let have: HashMap<String, (bool, bool, String)> = uow
            .reads()
            .agents()
            .signals(sid)?
            .into_iter()
            .map(|s| (s.external_id, (s.seen, s.auto_seen, s.session)))
            .collect();
        let wanted: HashSet<&str> = sigs.iter().map(|s| s.ext.as_str()).collect();
        let mut batch = Batch::default();
        let mut fresh = vec![];
        for s in &sigs {
            let prev = have.get(&s.ext);
            // New, or back after we resolved it: new signal. Dismissed by you: stays out.
            let bump = prev.is_none_or(|p| p.0 && p.1);
            if prev.is_none() {
                fresh.push(s);
            }
            let mut info = s.info.clone();
            info["why"] = json!(s.why);
            batch.items.push(ItemUpsert {
                external_id: s.ext.clone(),
                kind: s.kind.clone(),
                title: format!("{} {}", s.name, s.reason),
                body: s.context.clone(),
                occurred_at: Ts::parse(&s.at).unwrap_or_default(),
                ended_at: s.ended.as_deref().and_then(Ts::parse),
                status: "open".into(),
                msg_count: Some(s.messages.iter().filter(|m| !m.3).count() as i64),
                raw_json: json!({ "agent": info }),
                messages: s
                    .messages
                    .iter()
                    .map(|(at, who, text, faded)| Message {
                        at: Ts::parse(at).unwrap_or_default(),
                        author: who.clone(),
                        text: text.clone(),
                        author_id: None,
                        context: *faded,
                    })
                    .collect(),
                bump,
                ..Default::default()
            });
        }
        sync::apply(uow, &source, &batch, app.clock.now())?;
        // No longer true (answered, replied to, delivered, restarted): out of Incoming, marked as ours.
        for (ext, (seen, _, session)) in &have {
            if !*seen && !wanted.contains(ext.as_str()) && !unknown.contains(session) {
                uow.agents().auto_seen(sid, ext)?;
            }
        }
        file(uow, sid, &fresh)
    })?;
    if needs_guess {
        suggest_later(app);
    }
    Ok(true)
}

/// A new signal's topic: its ticket's, else the one its session's other signals
/// are filed under. Returns whether any is left for the AI to guess.
fn file(uow: &dyn UnitOfWork, sid: i64, fresh: &[&Sig]) -> Result<bool> {
    let r = uow.reads();
    let mut guess = false;
    for s in fresh {
        let Some(item) = uow.items().find(sid, &s.ext)? else {
            continue;
        };
        let by_ticket = match &s.ticket {
            Some(k) => r.agents().ticket_topic(k)?.map(|t| (t, format!("ticket {k}"))),
            None => None,
        };
        let topic = match by_ticket {
            Some(t) => Some(t),
            None => r
                .agents()
                .session_topic(sid, item, &s.session)?
                .map(|t| (t, "same session".to_owned())),
        };
        match topic {
            Some((t, why)) => {
                uow.links().suggest_if_unlinked(item, t, 0.9, &why)?;
            }
            None => guess = true,
        }
    }
    Ok(guess)
}

/// The small tier's topic guesses for what's left, at most once a minute, off the sync thread.
fn suggest_later(app: &App) {
    let now = std::time::Instant::now();
    {
        let mut last = app.agents.incoming_guess.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(60)) {
            return;
        }
        *last = Some(now);
    }
    let Some(app) = app.arc() else { return };
    std::thread::spawn(move || {
        if let Err(e) = crate::ai::call(&app, "suggest_topics", &json!({ "limit": 10 })) {
            eprintln!("agents: topic suggestions: {e}");
        }
    });
}

/// Keep Incoming in step with the store, about once a second while it changes.
pub fn start(app: std::sync::Arc<App>) {
    std::thread::Builder::new()
        .name("agents-incoming".into())
        .spawn(move || {
            // Let restored sessions and the first Hub poll land before judging what's resolved.
            std::thread::sleep(std::time::Duration::from_secs(5));
            let mut seen_rev = u64::MAX;
            loop {
                let rev = app.agents.store.lock().rev;
                if rev != seen_rev {
                    seen_rev = rev;
                    if let Err(e) = sync(&app) {
                        eprintln!("agents: incoming sync: {e}");
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::{Fact, Request, Who};
    use crate::ai::context::tests::app;
    use rusqlite::OptionalExtension;

    fn session(app: &App, id: &str, source: &str) {
        let s = Session {
            id: id.into(),
            source: source.into(),
            name: format!("{id}-name"),
            cwd: "/r".into(),
            status: Status::Running,
            ..Default::default()
        };
        app.agents.store.lock().put_session(s);
    }

    fn you(app: &App, id: &str, text: &str) {
        app.agents.store.lock().push_row(
            &format!("s:{id}"),
            Row {
                at: crate::time::t("2026-10-03T10:00:00").to_db(),
                kind: Kind::You,
                who: Some(Who::you()),
                text: text.into(),
                ..Default::default()
            },
        );
    }

    fn approval(app: &App, id: &str) {
        let r = Request {
            id: format!("{id}:7"),
            session_id: id.into(),
            thread: format!("s:{id}"),
            kind: "approval".into(),
            at: crate::time::t("2026-10-03T10:01:00").to_db(),
            command: "rm -rf build".into(),
            ..Default::default()
        };
        app.agents.store.lock().put_request(r);
    }

    fn seen(app: &App, ext: &str) -> Option<(i64, i64)> {
        app.db()
            .query_row(
                "SELECT seen, coalesce(json_extract(raw_json, '$.auto_seen'), 0) FROM items WHERE external_id = ?1",
                [ext],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    #[test]
    fn an_approval_leaves_when_answered_and_comes_back_if_asked_again() {
        let (app, _d) = app();
        session(&app, "acp-1", "acp");
        you(&app, "acp-1", "Clean the build");
        approval(&app, "acp-1");
        assert!(sync(&app).unwrap());
        assert_eq!(seen(&app, "req:acp-1:7"), Some((0, 0)));
        let (title, msgs): (String, i64) = app.db().query_row("SELECT title, (SELECT count(*) FROM item_messages m WHERE m.item_id = items.id) FROM items WHERE external_id = 'req:acp-1:7'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(
            (title.as_str(), msgs),
            ("acp-1-name needs approval", 1),
            "your last message is the conversation"
        );
        // Answered in Agents: out of Incoming, as ours.
        app.agents.store.lock().remove_request("acp-1:7");
        sync(&app).unwrap();
        assert_eq!(seen(&app, "req:acp-1:7"), Some((1, 1)));
        // Rebuilt (an app restart): back.
        approval(&app, "acp-1");
        sync(&app).unwrap();
        assert_eq!(seen(&app, "req:acp-1:7").map(|s| s.0), Some(0));
    }

    #[test]
    fn dismissing_it_sticks_while_it_waits() {
        let (app, _d) = app();
        session(&app, "acp-1", "acp");
        approval(&app, "acp-1");
        sync(&app).unwrap();
        app.db()
            .execute("UPDATE items SET seen = 1 WHERE external_id = 'req:acp-1:7'", [])
            .unwrap();
        app.agents
            .store
            .lock()
            .update_session("acp-1", |s| s.status = Status::Waiting);
        sync(&app).unwrap();
        assert_eq!(seen(&app, "req:acp-1:7"), Some((1, 0)));
    }

    #[test]
    fn a_turn_end_is_one_signal_until_you_reply() {
        let (app, _d) = app();
        session(&app, "acp-1", "acp");
        you(&app, "acp-1", "Fix resume");
        app.agents.store.lock().push_row(
            "s:acp-1",
            Row {
                at: crate::time::t("2026-10-03T10:05:00").to_db(),
                kind: Kind::Output,
                text: "Resume works now.".into(),
                ..Default::default()
            },
        );
        app.agents.store.lock().update_session("acp-1", |s| {
            s.status = Status::Idle;
            s.stats.turns = 1;
        });
        sync(&app).unwrap();
        assert_eq!(seen(&app, "turn:acp-1:1"), Some((0, 0)));
        you(&app, "acp-1", "Add a test");
        app.agents
            .store
            .lock()
            .update_session("acp-1", |s| s.status = Status::Running);
        sync(&app).unwrap();
        assert_eq!(seen(&app, "turn:acp-1:1"), Some((1, 1)));
    }

    #[test]
    fn a_session_whose_transcript_isnt_loaded_is_left_alone() {
        let (app, _d) = app();
        session(&app, "acp-1", "acp");
        you(&app, "acp-1", "Fix resume");
        app.agents.store.lock().push_row(
            "s:acp-1",
            Row {
                at: crate::time::t("2026-10-03T10:05:00").to_db(),
                kind: Kind::Output,
                text: "Done.".into(),
                ..Default::default()
            },
        );
        app.agents.store.lock().update_session("acp-1", |s| {
            s.status = Status::Idle;
            s.stats.turns = 1;
        });
        sync(&app).unwrap();
        // A new run of the app: the session is known, its rows aren't read yet.
        app.agents.store.lock().threads.get_mut("s:acp-1").unwrap().loaded = false;
        app.agents
            .store
            .lock()
            .update_session("acp-1", |s| s.title = "x".into());
        sync(&app).unwrap();
        assert_eq!(seen(&app, "turn:acp-1:1"), Some((0, 0)));
    }

    #[test]
    fn a_ticket_files_it_under_the_tickets_topic() {
        let (app, _d) = app();
        let conn = app.db();
        conn.execute(
            "INSERT INTO sources (kind, tag, name, config_json, enabled) VALUES ('jira', 'work', 'Jira', '{}', 1)",
            [],
        )
        .unwrap();
        let jira = conn.last_insert_rowid();
        conn.execute("INSERT INTO items (source_id, external_id, kind, title, occurred_at) VALUES (?1, 'ING-31', 'issue', 'Webhooks', '2026-10-03T09:00:00.000Z')", [jira]).unwrap();
        let item = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO topics (name, category) VALUES ('Jira webhooks', 'work')",
            [],
        )
        .unwrap();
        let topic = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')",
            [item, topic],
        )
        .unwrap();
        drop(conn);
        session(&app, "scion:h:a", "scion");
        app.agents.store.lock().update_session("scion:h:a", |s| {
            s.facts.ticket = Some(Fact {
                label: "ING-31".into(),
                ..Default::default()
            })
        });
        approval(&app, "scion:h:a");
        sync(&app).unwrap();
        let linked: (i64, String) = app
            .db()
            .query_row("SELECT tl.topic_id, tl.state FROM topic_links tl JOIN items i ON i.id = tl.item_id WHERE i.external_id = 'req:scion:h:a:7'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(linked, (topic, "suggested".into()));
        let q = crate::queries::incoming::queue(&crate::store::over(&app.db()), &app.clock).unwrap();
        let t = q.topics.iter().find(|t| t.id == topic).unwrap();
        assert!(t
            .signals
            .iter()
            .any(|s| s.source == "agents" && s.agent.as_ref().is_some_and(|a| a["reason"] == "needs approval")));
        assert_eq!(q.agents.len(), 1, "and under Agents waiting");
        assert_eq!(q.agents[0].topic.as_deref(), Some("Jira webhooks"));
    }
}

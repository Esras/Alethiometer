//! Archiving agents (design "Agents Archive" 01–05; rules in app/docs/agents.md › Archive).

use super::model::{Group, Row, Sandbox, Session, Status};
use super::store::Inner;
use crate::domain::agents::Archived;
use crate::error::{msg, Result};
use crate::state::App;
use crate::time::Ts;
use serde::Serialize;
use serde_json::{json, Value};

pub const DELETE_OP: &str = "scion.delete_agent";
const NUDGE_SNOOZE_DAYS: i64 = 7;

#[derive(Debug, Serialize, PartialEq)]
pub struct Plan {
    pub target: String,
    /// session | group
    pub kind: &'static str,
    pub name: String,
    /// "in Webhooks · ingest-svc", "ingest-svc", "Unsorted".
    pub place: String,
    /// ok | not_done | blocked
    pub state: &'static str,
    pub rows: Vec<PlanRow>,
    /// Not done: why, unpushed commits first.
    pub reasons: Vec<Reason>,
    /// Blocked: how many members are still going.
    pub going: usize,
    /// Hub agents deleted, local sessions stopped, OpenShell sandboxes deleted.
    pub hub: usize,
    pub local: usize,
    pub sandboxes: usize,
    #[serde(skip)]
    pub doomed: Vec<Sandbox>,
    #[serde(skip)]
    pub sessions: Vec<String>,
    #[serde(skip)]
    pub group: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PlanRow {
    /// group | room | session
    pub kind: &'static str,
    pub name: String,
    pub session: Option<String>,
    pub status: Option<Status>,
    pub harness: String,
    pub depth: usize,
    pub lit: bool,
    /// will archive | unchanged | ready | running | waiting for you | "" (a lit group's members)
    pub tag: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Reason {
    pub text: String,
    pub note: String,
}

/// `g:<group>`, `r:<room>` (its group) or `s:<session>` (with its subagents).
pub fn plan(i: &Inner, target: &str) -> Result<Plan> {
    let gone = || msg("That's gone from the tree");
    let (group, top): (Option<&Group>, Option<&Session>) = match target.split_once(':') {
        Some(("g", id)) => (Some(i.groups.get(id).ok_or_else(gone)?), None),
        Some(("r", id)) => (Some(i.groups.values().find(|g| g.room.as_ref().is_some_and(|r| r.id == id)).ok_or_else(gone)?), None),
        Some(("s", id)) => {
            let s = i.sessions.get(id).ok_or_else(gone)?;
            (s.group_id.as_ref().and_then(|g| i.groups.get(g)), Some(s))
        }
        _ => return Err(msg("Pick a session, room or group")),
    };
    let roots: Vec<&Session> = match group {
        Some(g) => ordered(i, |s| s.group_id.as_deref() == Some(g.id.as_str()) && s.parent_id.is_none()),
        None => top.into_iter().collect(),
    };
    let mut shown: Vec<(&Session, usize)> = vec![];
    for r in roots {
        shown.push((r, 0));
        descendants(i, &r.id, 1, &mut shown);
    }
    let archived: Vec<&Session> = match top {
        Some(t) => {
            let mut v = vec![(t, 0)];
            descendants(i, &t.id, 1, &mut v);
            v.into_iter().map(|(s, _)| s).collect()
        }
        None => shown.iter().map(|(s, _)| *s).collect(),
    };
    let ids: Vec<&str> = archived.iter().map(|s| s.id.as_str()).collect();
    let asking = |s: &Session| s.status == Status::Waiting || i.requests.values().any(|r| r.session_id == s.id);
    let busy = |s: &Session| s.status == Status::Running || asking(s);
    let going = archived.iter().filter(|s| busy(s)).count();
    let reasons = if going == 0 { not_done(&archived) } else { vec![] };
    let doomed = doomed(i, &archived);
    let state = if going > 0 {
        "blocked"
    } else if reasons.is_empty() {
        "ok"
    } else {
        "not_done"
    };
    let whole = top.is_none();
    let tag = |lit: bool, s: Option<&Session>| -> &'static str {
        match (state, s) {
            ("blocked", Some(s)) if asking(s) => "waiting for you",
            ("blocked", Some(s)) if busy(s) => "running",
            ("blocked", _) => "ready",
            _ if !lit => "unchanged",
            _ if whole => "",
            _ => "will archive",
        }
    };
    let mut rows = vec![];
    if let Some(g) = group {
        let head = if whole && state != "blocked" { "will archive" } else { tag(whole, None) };
        rows.push(PlanRow { kind: "group", name: g.name.clone(), session: None, status: None, harness: String::new(), depth: 0, lit: whole, tag: head });
        if let Some(room) = &g.room {
            rows.push(PlanRow { kind: "room", name: room.name.clone(), session: None, status: None, harness: String::new(), depth: 0, lit: whole, tag: tag(whole, None) });
        }
    }
    for (s, depth) in &shown {
        let lit = ids.contains(&s.id.as_str());
        rows.push(PlanRow { kind: "session", name: s.name.clone(), session: Some(s.id.clone()), status: Some(s.status), harness: s.harness.clone(), depth: *depth, lit, tag: tag(lit, Some(s)) });
    }
    let (kind, name, place) = match (top, group) {
        (Some(s), Some(g)) => ("session", s.name.clone(), format!("in {} · {}", g.name, g.project)),
        (Some(s), None) => ("session", s.name.clone(), "Unsorted".to_owned()),
        (None, Some(g)) => ("group", g.name.clone(), g.project.clone()),
        (None, None) => unreachable!("a target is a session or a group"),
    };
    Ok(Plan {
        target: target.to_owned(),
        kind,
        name,
        place,
        state,
        rows,
        reasons,
        going,
        hub: archived.iter().filter(|s| s.source == "scion").count(),
        local: archived.iter().filter(|s| s.source == "acp").count(),
        sandboxes: doomed.len(),
        doomed,
        sessions: ids.iter().map(|s| s.to_string()).collect(),
        group: if whole { group.map(|g| g.id.clone()) } else { None },
    })
}

fn ordered(i: &Inner, f: impl Fn(&Session) -> bool) -> Vec<&Session> {
    let mut v: Vec<&Session> = i.sessions.values().filter(|s| f(s)).collect();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    v
}

fn descendants<'a>(i: &'a Inner, id: &str, depth: usize, out: &mut Vec<(&'a Session, usize)>) {
    for c in ordered(i, |s| s.parent_id.as_deref() == Some(id)) {
        out.push((c, depth));
        if depth < 8 {
            descendants(i, &c.id, depth + 1, out);
        }
    }
}

/// The OpenShell sandboxes archiving these deletes: ones Alethiometer made
/// (from a directory here), that no session staying in the tree runs in.
/// A sandbox that was already there is never deleted.
fn doomed(i: &Inner, archived: &[&Session]) -> Vec<Sandbox> {
    let ids: Vec<&str> = archived.iter().map(|s| s.id.as_str()).collect();
    let mut out: Vec<Sandbox> = vec![];
    for sb in archived.iter().filter_map(|s| s.sandbox.as_ref()).filter(|b| !b.upload.is_empty()) {
        let same = |b: &Sandbox| b.gateway == sb.gateway && b.name == sb.name;
        let kept = i.sessions.values().any(|s| !ids.contains(&s.id.as_str()) && s.sandbox.as_ref().is_some_and(same));
        if !kept && !out.iter().any(same) {
            out.push(Sandbox { allow_all: false, synced_turns: 0, ..sb.clone() });
        }
    }
    out
}

/// Why stopped Scion agents aren't done, and sandboxed work that hasn't been
/// brought back (its sandbox goes); other local sessions lose nothing and
/// have no reliable "done" yet (ISSUES.md).
fn not_done(sessions: &[&Session]) -> Vec<Reason> {
    let scion: Vec<&&Session> = sessions.iter().filter(|s| s.source == "scion").collect();
    let mut out = vec![];
    for s in sessions.iter().filter(|s| s.parent_id.is_none()) {
        if let Some(sb) = s.sandbox.as_ref().filter(|b| !b.upload.is_empty() && s.stats.turns > b.synced_turns) {
            let n = s.stats.turns - sb.synced_turns;
            out.push(Reason { text: format!("{}'s changes in sandbox {} haven't been brought back ({n} turn{} since)", s.name, sb.name, if n == 1 { "" } else { "s" }), note: "lost with the sandbox".into() });
        }
    }
    for s in &scion {
        if let Some(n) = s.facts.branch.as_ref().and_then(|b| b.detail.strip_suffix(" ahead")).and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0) {
            out.push(Reason { text: format!("{} has {n} unpushed commit{}", s.name, if n == 1 { "" } else { "s" }), note: "lost with its clone".into() });
        }
    }
    let mut seen = std::collections::HashSet::new();
    for s in &scion {
        if let Some(p) = s.facts.pr.as_ref().filter(|p| !p.done && p.detail != "closed" && seen.insert(p.label.clone())) {
            out.push(Reason { text: format!("PR {} is still open", p.label), note: "stays on GitHub".into() });
        }
    }
    for s in &scion {
        if let Some(t) = s.facts.ticket.as_ref().filter(|t| !t.done && seen.insert(t.label.clone())) {
            let text = if t.detail.is_empty() { format!("{} isn't done", t.label) } else { format!("{} is {}", t.label, t.detail) };
            out.push(Reason { text, note: "stays in Jira".into() });
        }
    }
    for s in &scion {
        match s.status {
            Status::Failed => out.push(Reason { text: format!("{} failed", s.name), note: String::new() }),
            Status::Idle => out.push(Reason { text: format!("{} stopped before it completed", s.name), note: String::new() }),
            _ => {}
        }
    }
    out
}

/// Archive what `target` names; `force` is Archive anyway.
pub fn archive(app: &App, target: &str, force: bool) -> Result<Value> {
    let p = plan(&app.agents.store.lock(), target)?;
    match p.state {
        "blocked" => return Err(msg(format!("{} still has {} going. Stop {}, then archive it.", p.name, agents(p.going), if p.going == 1 { "it" } else { "them" }))),
        "not_done" if !force => return Err(msg(format!("{} isn't done: Archive anyway archives it as it is", p.name))),
        _ => {}
    }
    for id in p.sessions.iter().filter(|id| id.starts_with("acp-")) {
        super::acp::stop(app, id);
    }
    let groups: Vec<String> = p.group.iter().cloned().collect();
    let cli = app.settings().agents.openshell.command.clone();
    app.agents.store.archive(app.store(), &groups, &p.sessions, app.clock.now(), |uow, items| {
        if !p.doomed.is_empty() {
            let source = super::incoming::source_id(uow)?;
            for sb in &p.doomed {
                crate::outbox::request(uow, source, super::openshell::DELETE_OP, &json!({ "command": cli, "gateway": sb.gateway, "name": sb.name }), None)?;
            }
        }
        for a in items.iter().filter(|a| a.kind == "session" && a.source == "scion") {
            let s: Session = serde_json::from_str(&a.data).map_err(|e| msg(e.to_string()))?;
            let hub = a.id.strip_prefix("scion:").and_then(|r| r.split_once(':')).map(|(h, _)| h).unwrap_or_default();
            let source = uow.reads().sources().find("scion", hub)?.ok_or_else(|| msg(format!("Scion Hub '{hub}' isn't a source any more")))?;
            crate::outbox::request(uow, source, DELETE_OP, &json!({ "agent": s.ext, "name": s.name }), None)?;
        }
        Ok(())
    })?;
    if p.hub > 0 || p.sandboxes > 0 {
        crate::outbox::wake();
    }
    Ok(json!({ "ok": true, "hub": p.hub, "local": p.local, "sandboxes": p.sandboxes }))
}

fn agents(n: usize) -> String {
    format!("{n} agent{}", if n == 1 { "" } else { "s" })
}

/// The done nudge (01): a Scion group whose sessions all completed and whose PR merged (or ticket is Done).
pub fn nudge(i: &Inner, g: &Group, now: Ts) -> Option<String> {
    if g.source != "scion" || i.snoozed.get(&g.id).is_some_and(|until| *until > now) {
        return None;
    }
    let members: Vec<&Session> = i.sessions.values().filter(|s| s.group_id.as_deref() == Some(g.id.as_str())).collect();
    if members.is_empty() || members.iter().any(|s| s.status != Status::Done) {
        return None;
    }
    let pr = members.iter().find_map(|s| s.facts.pr.as_ref().filter(|p| p.done)).map(|p| format!("{} merged", p.label));
    let ticket = || members.iter().find_map(|s| s.facts.ticket.as_ref().filter(|t| t.done)).map(|t| format!("{} Done", t.label));
    pr.or_else(ticket).map(|done| format!("completed, {done}"))
}

/// Not now: hide a group's nudge for 7 days.
pub fn snooze(app: &App, group: &str) -> Result<()> {
    let until = app.clock.now() + jiff::SignedDuration::from_hours(24 * NUDGE_SNOOZE_DAYS);
    app.store().write(|uow| uow.agents().snooze_nudge(group, until))?;
    let mut i = app.agents.store.lock();
    i.snoozed.insert(group.to_owned(), until);
    i.touch_tree();
    Ok(())
}

/// Count the failed deletes (Hub agents, sandboxes) again (after a send, and at startup).
pub fn refresh_failed(app: &App) {
    let Ok(n) = app.store().read(|r| Ok(r.outbox().failed(DELETE_OP)? + r.outbox().failed(super::openshell::DELETE_OP)?)) else { return };
    let mut i = app.agents.store.lock();
    if i.hub_failed != n {
        i.hub_failed = n;
        i.touch_tree();
    }
}

/// Archived items matching the tree filter, newest first, each with the line that matched.
pub fn search(app: &App, q: &str) -> Result<Value> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(json!({ "items": [], "total": 0 }));
    }
    let (found, total) = app.store().read(|r| Ok((r.agents().archived_matching(q, 60)?, r.agents().archived_sessions()?)))?;
    let items: Vec<Value> = found
        .into_iter()
        .map(|m| {
            let mut v = brief(&m.item);
            if let Some(row) = m.row.and_then(|(_, data)| serde_json::from_str::<Row>(&data).ok()) {
                v["line"] = json!(snippet(&row_text(&row), q));
                v["who"] = json!(row.who.map(|w| w.name).unwrap_or_else(|| v["name"].as_str().unwrap_or("").to_owned()));
            }
            v
        })
        .collect();
    Ok(json!({ "items": items, "total": total }))
}

fn brief(a: &Archived) -> Value {
    let data: Value = serde_json::from_str(&a.data).unwrap_or_default();
    json!({
        "id": a.id,
        "kind": a.kind,
        "source": a.source,
        "project": a.project,
        "group": a.group_name,
        "group_id": a.group_id,
        "at": a.at,
        "name": data["name"],
        "goal": data["goal"],
        "harness": data["harness"],
        "model": data["model"],
        "room": data["room"]["name"],
    })
}

fn row_text(r: &Row) -> String {
    match &r.tool {
        Some(t) => [r.text.as_str(), t.detail.as_str(), t.result.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" "),
        None => r.text.clone(),
    }
}

/// The text around the first match of `q`, on one line.
pub fn snippet(text: &str, q: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(at) = flat.to_ascii_lowercase().find(&q.to_ascii_lowercase()) else { return crate::ai::context::clip(&flat, 120) };
    let start = flat[..at].char_indices().rev().nth(40).map_or(0, |(i, _)| i);
    let end = flat[at..].char_indices().nth(q.chars().count() + 80).map_or(flat.len(), |(i, _)| at + i);
    format!("{}{}{}", if start > 0 { "…" } else { "" }, flat[start..end].trim(), if end < flat.len() { "…" } else { "" })
}

/// An archived item opened read-only: what it was, its rows, and a group's members.
pub fn open(app: &App, id: &str) -> Result<Value> {
    app.store().read(|r| {
        let a = r.agents().archived(id)?.ok_or_else(|| msg("Nothing archived under that name"))?;
        let data: Value = serde_json::from_str(&a.data).unwrap_or_default();
        let thread = if a.kind == "session" { super::session_thread(&a.id) } else { data["room"]["id"].as_str().map(|r| format!("r:{r}")).unwrap_or_default() };
        let rows = if thread.is_empty() { vec![] } else { r.agents().transcript(&thread)? };
        let members = if a.kind == "group" { r.agents().archived_members(&a.id)? } else { vec![] };
        let mut messages = rows.len();
        for m in &members {
            messages += r.agents().transcript(&super::session_thread(&m.id))?.len();
        }
        Ok(json!({
            "item": brief(&a),
            "data": data,
            "thread": thread,
            "rows": rows,
            "members": members.iter().map(brief).collect::<Vec<_>>(),
            "kept": { "messages": messages, "transcripts": if a.kind == "session" { 1 } else { members.len() } },
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::{Fact, Facts, Room};

    fn session(id: &str, group: &str, status: Status) -> Session {
        let source = if id.starts_with("acp-") { "acp" } else { "scion" };
        let at = "2026-10-03T09:00:00.000Z".to_owned();
        Session { id: id.into(), name: id.rsplit(':').next().unwrap().into(), source: source.into(), group_id: Some(group.into()), status, active_at: at.clone(), created_at: at, ..Default::default() }
    }

    fn inner() -> Inner {
        let mut i = Inner::default();
        let g = "scion:h:p:c1";
        let room = Room { id: "c1".into(), name: "webhooks".into(), ..Default::default() };
        i.put_group(Group { id: g.into(), source: "scion".into(), project: "ingest-svc".into(), name: "Webhooks".into(), room: Some(room), ..Default::default() });
        i.put_session(session("scion:h:planner", g, Status::Running));
        i.put_session(session("scion:h:schema-diff", g, Status::Done));
        i
    }

    #[test]
    fn a_done_session_archives_alone_and_the_group_stays() {
        let i = inner();
        let p = plan(&i, "s:scion:h:schema-diff").unwrap();
        assert_eq!((p.state, p.kind, p.hub), ("ok", "session", 1));
        assert_eq!(p.sessions, vec!["scion:h:schema-diff"]);
        assert_eq!(p.group, None);
        let tags: Vec<(&str, &str)> = p.rows.iter().map(|r| (r.name.as_str(), r.tag)).collect();
        assert_eq!(tags, vec![("Webhooks", "unchanged"), ("webhooks", "unchanged"), ("planner", "unchanged"), ("schema-diff", "will archive")]);
    }

    #[test]
    fn a_group_with_a_member_running_cant_be_archived() {
        let i = inner();
        for target in ["g:scion:h:p:c1", "r:c1"] {
            let p = plan(&i, target).unwrap();
            assert_eq!((p.state, p.going), ("blocked", 1), "{target}");
            let tags: Vec<&str> = p.rows.iter().map(|r| r.tag).collect();
            assert_eq!(tags, vec!["ready", "ready", "running", "ready"]);
        }
    }

    #[test]
    fn a_stopped_scion_group_lists_why_it_isnt_done() {
        let mut i = inner();
        i.update_session("scion:h:planner", |s| {
            s.status = Status::Done;
            s.facts = Facts {
                branch: Some(Fact { label: "feat/x".into(), detail: "2 ahead".into(), ..Default::default() }),
                pr: Some(Fact { label: "#212".into(), ..Default::default() }),
                ticket: Some(Fact { label: "SE-21".into(), detail: "In Progress".into(), ..Default::default() }),
                ..Default::default()
            };
        });
        i.update_session("scion:h:schema-diff", |s| s.status = Status::Failed);
        let p = plan(&i, "g:scion:h:p:c1").unwrap();
        assert_eq!(p.state, "not_done");
        let why: Vec<&str> = p.reasons.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(why, vec!["planner has 2 unpushed commits", "PR #212 is still open", "SE-21 is In Progress", "schema-diff failed"]);
        assert_eq!(p.group.as_deref(), Some("scion:h:p:c1"));
        assert_eq!(p.sessions.len(), 2);
    }

    #[test]
    fn local_sessions_never_ask_for_archive_anyway() {
        let mut i = Inner::default();
        i.put_session(Session { id: "acp-1".into(), source: "acp".into(), status: Status::Idle, ..Default::default() });
        let p = plan(&i, "s:acp-1").unwrap();
        assert_eq!((p.state, p.place.as_str(), p.local), ("ok", "Unsorted", 1));
    }

    #[test]
    fn the_nudge_needs_every_session_completed_and_the_pr_merged() {
        let mut i = inner();
        let g = i.groups["scion:h:p:c1"].clone();
        let now = crate::time::t("2026-10-03T12:00:00");
        assert_eq!(nudge(&i, &g, now), None);
        i.update_session("scion:h:planner", |s| s.status = Status::Done);
        assert_eq!(nudge(&i, &g, now), None, "no PR or ticket says it's done");
        i.update_session("scion:h:planner", |s| s.facts.pr = Some(Fact { label: "#208".into(), detail: "merged".into(), done: true, ..Default::default() }));
        assert_eq!(nudge(&i, &g, now).as_deref(), Some("completed, #208 merged"));
        i.snoozed.insert(g.id.clone(), now + jiff::SignedDuration::from_hours(1));
        assert_eq!(nudge(&i, &g, now), None, "Not now");
    }

    #[test]
    fn snippets_show_the_text_around_the_match() {
        let long = format!("{} three deliveries reached us only after a retry. {}", "x ".repeat(40), "y ".repeat(60));
        let s = snippet(&long, "RETRY");
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("after a retry."), "{s}");
        assert_eq!(snippet("a retry", "retry"), "a retry");
    }

    fn app() -> (crate::state::App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::state::App::from_parts(crate::db::open_in_memory().unwrap(), crate::settings::Settings::default(), dir.path()).unwrap();
        let cfg = json!({ "hub_url": "http://hub.test", "token_secret": "scion.token" }).to_string();
        app.store().write(|uow| uow.sources().register("scion", "h", "work", &cfg, true)).unwrap();
        app.platform.secret_set("scion.token", "scion_pat_test").unwrap();
        (app, dir)
    }

    #[test]
    fn archiving_a_scion_session_keeps_its_thread_and_deletes_it_on_the_hub() {
        let (app, _d) = app();
        {
            let mut i = app.agents.store.lock();
            *i = inner();
            i.update_session("scion:h:schema-diff", |s| s.ext = "a-77".into());
            i.set_rows("s:scion:h:schema-diff", vec![Row { ext: "m1".into(), text: "Three deliveries reached us only after a retry.".into(), ..Default::default() }]);
        }
        archive(&app, "s:scion:h:schema-diff", false).unwrap();
        {
            let mut i = app.agents.store.lock();
            assert!(!i.sessions.contains_key("scion:h:schema-diff"));
            assert!(i.sessions.contains_key("scion:h:planner"), "the group stays");
            i.put_session(session("scion:h:schema-diff", "scion:h:p:c1", Status::Done));
            assert!(!i.sessions.contains_key("scion:h:schema-diff"), "the Hub listing it again doesn't bring it back");
        }
        let found = search(&app, "RETRY").unwrap();
        assert_eq!(found["items"][0]["id"], "scion:h:schema-diff");
        assert_eq!(found["items"][0]["group"], "Webhooks");
        assert_eq!(found["items"][0]["line"], "Three deliveries reached us only after a retry.");
        let opened = open(&app, "scion:h:schema-diff").unwrap();
        assert_eq!(opened["rows"].as_array().unwrap().len(), 1);

        let http = crate::sync::http::FakeHttp::default();
        http.on("DELETE", "/api/v1/agents/a-77", 404, r#"{"error":{"code":"not_found","message":"gone"}}"#);
        assert!(crate::outbox::process_one(&app, &http).unwrap());
        assert_eq!(http.requests()[0].0, "DELETE");
        let rows = app.store().read(|r| r.outbox().recent(app.clock.now() - jiff::SignedDuration::from_hours(1), 10)).unwrap();
        assert_eq!((rows[0].op.as_str(), rows[0].state.as_str()), (DELETE_OP, "confirmed"), "already gone counts as deleted");
    }

    #[test]
    fn a_failed_hub_delete_leaves_it_archived_and_is_counted() {
        let (app, _d) = app();
        *app.agents.store.lock() = inner();
        archive(&app, "s:scion:h:schema-diff", false).unwrap();
        let http = crate::sync::http::FakeHttp::default();
        http.on("DELETE", "/api/v1/agents/", 502, r#"{"error":{"code":"runtime_error","message":"broker down"}}"#);
        crate::outbox::process_one(&app, &http).unwrap();
        let i = app.agents.store.lock();
        assert_eq!(i.hub_failed, 1);
        assert!(i.archived.contains("scion:h:schema-diff"));
    }

    #[test]
    fn an_archived_local_session_doesnt_load_again() {
        let (app, _d) = app();
        app.agents.store.lock().put_session(Session { id: "acp-1".into(), source: "acp".into(), name: "header-fix".into(), ..Default::default() });
        app.agents.store.lock().push_row("s:acp-1", Row { text: "fix the settings header".into(), ..Default::default() });
        app.agents.store.flush(app.store()).unwrap();
        archive(&app, "s:acp-1", false).unwrap();
        let back = crate::agents::store::Store::default();
        back.load(app.store()).unwrap();
        assert!(back.lock().sessions.is_empty());
        assert!(back.lock().archived.contains("acp-1"));
        assert_eq!(search(&app, "settings header").unwrap()["items"][0]["id"], "acp-1");
    }

    #[test]
    fn a_running_session_is_refused_at_archive_time() {
        let (app, _d) = app();
        *app.agents.store.lock() = inner();
        let e = archive(&app, "s:scion:h:planner", true).unwrap_err().to_string();
        assert!(e.contains("Stop it"), "{e}");
        assert!(app.agents.store.lock().sessions.contains_key("scion:h:planner"));
    }

    #[test]
    fn an_archived_sessions_signals_leave_incoming() {
        let (app, _d) = app();
        *app.agents.store.lock() = inner();
        crate::agents::incoming::sync(&app).unwrap();
        let sid = app.store().write(|uow| crate::agents::incoming::source_id(uow)).unwrap();
        let signals = || app.store().read(|r| r.agents().signals(sid)).unwrap();
        assert!(signals().iter().any(|s| s.external_id == "done:scion:h:schema-diff" && !s.seen));
        archive(&app, "s:scion:h:schema-diff", false).unwrap();
        crate::agents::incoming::sync(&app).unwrap();
        assert!(signals().iter().any(|s| s.external_id == "done:scion:h:schema-diff" && s.seen && s.auto_seen));
    }

    #[test]
    fn a_hub_poll_racing_the_archive_cant_bring_the_session_back() {
        let (app, _d) = app();
        *app.agents.store.lock() = inner();
        let stop = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|t| {
            t.spawn(|| {
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    let mut i = app.agents.store.lock();
                    i.put_session(session("scion:h:schema-diff", "scion:h:p:c1", Status::Done));
                    i.set_rows("s:scion:h:schema-diff", vec![Row { ext: "m1".into(), text: "hi".into(), ..Default::default() }]);
                }
            });
            archive(&app, "s:scion:h:schema-diff", false).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let i = app.agents.store.lock();
        assert!(!i.sessions.contains_key("scion:h:schema-diff"));
        assert!(!i.threads.contains_key("s:scion:h:schema-diff"));
    }
}

//! Todos (22a, 22c): items you own in a system that holds todos; see docs/todos.md.

use super::{arg_i64, arg_str, due_label, opt_i64, parse_date, TopicRef};
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::items::NewItem;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::domain::sources::SourceRow;
use crate::error::{msg, Result};
use crate::outbox::request;
use crate::time::{Date, Ts};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalExt {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub from_item: Option<i64>,
    #[serde(default)]
    pub notes_line_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JiraExt {
    pub key: String,
    pub status: String,
    pub issue_type: Option<String>,
}

impl JiraExt {
    pub fn is_epic(&self) -> bool {
        self.issue_type.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("epic"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObsidianExt {
    pub path: String,
    pub text: String,
    #[serde(default)]
    pub line: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TodoExt {
    Local(LocalExt),
    Jira(JiraExt),
    Obsidian(ObsidianExt),
}

#[derive(Deserialize)]
struct JiraRaw {
    key: String,
    fields: JiraFields,
}

#[derive(Deserialize)]
struct JiraFields {
    status: Named,
    #[serde(default)]
    issuetype: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

impl TodoExt {
    pub(crate) fn parse(source: &str, raw: Value) -> Option<TodoExt> {
        match source {
            "notes" => serde_json::from_value(raw).ok().map(TodoExt::Local),
            "jira" => serde_json::from_value::<JiraRaw>(raw).ok().map(|j| TodoExt::Jira(JiraExt { key: j.key, status: j.fields.status.name, issue_type: j.fields.issuetype.map(|t| t.name) })),
            "obsidian" => serde_json::from_value(raw).ok().map(TodoExt::Obsidian),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Estimate {
    pub minutes: i64,
    pub by: String,
}

#[derive(Debug, Serialize)]
pub struct Todo {
    pub item_id: i64,
    pub title: String,
    pub from: Option<String>,
    /// The deadline (a UTC instant; a day-only due is the end of that day here), for sorting.
    pub due: Option<Ts>,
    pub due_label: Option<String>,
    pub overdue: bool,
    pub topic: Option<TopicRef>,
    /// Place in the Priorities tab's Priority column, from 1 (`priorities.rs`).
    pub rank: Option<usize>,
    /// A local todo's scheduled day, and its label ("Thu").
    pub sched: Option<Date>,
    pub sched_label: Option<String>,
    pub url: Option<String>,
    pub estimate: Option<Estimate>,
    pub ext: TodoExt,
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "list" => Route::Read(|r, ctx, _| {
            let mut todos = list(r, ctx.clock)?;
            let ranks = super::priorities::ranks(r, ctx.clock, ctx.settings)?;
            for t in &mut todos {
                t.rank = ranks.get(&t.item_id).copied();
            }
            Ok(json!({ "todos": todos, "jira": jira_target(r)? }))
        }),
        "schedule" => Route::Write(|uow, _, args| {
            let day = args.get("sched").and_then(Value::as_str).map(str::trim).filter(|d| !d.is_empty()).map(parse_date).transpose()?;
            schedule(uow, arg_i64(args, "item_id")?, day)?;
            Ok(json!({ "ok": true }))
        }),
        "complete" | "reopen" => Route::Write(|uow, _, args| {
            complete(uow, arg_i64(args, "item_id")?, args.get("done").and_then(Value::as_bool).unwrap_or(true))?;
            Ok(json!({ "ok": true }))
        }),
        "create" => Route::Write(|uow, ctx, args| {
            create(uow, ctx.clock.now(), &Draft::from_args(args)?, opt_i64(args, "topic_id"), None)?;
            Ok(json!({ "ok": true }))
        }),
        "hold" => Route::Write(|uow, _, args| {
            let d = Draft::from_args(args)?;
            uow.todos().hold(arg_i64(args, "item_id")?, &Held { target: d.target, text: d.text, due: d.due, sched: d.sched })?;
            Ok(json!({ "ok": true }))
        }),
        "estimate" => Route::Write(|uow, _, args| {
            set_estimate(uow, arg_i64(args, "item_id")?, opt_i64(args, "minutes"))?;
            Ok(json!({ "ok": true }))
        }),
        "unhold" => Route::Write(|uow, _, args| {
            uow.todos().unhold(arg_i64(args, "item_id")?)?;
            Ok(json!({ "ok": true }))
        }),
        _ => return None,
    })
}

/// Open todos, soonest deadline first (undated last).
pub fn list(r: &dyn Reads, clock: &Clock) -> Result<Vec<Todo>> {
    let now = clock.now();
    let mut rows = r.todos().open()?;
    rows.sort_by_key(|t| (t.due.is_none(), t.due.map(|d| d.deadline(clock)), t.item_id));
    let skip = excluded_projects(r)?;
    let mut out = vec![];
    for t in rows {
        let Some(ext) = TodoExt::parse(&t.source_kind, serde_json::from_str(&t.raw_json).unwrap_or_default()) else { continue };
        if matches!(&ext, TodoExt::Jira(j) if project_excluded(&j.key, &skip)) {
            continue;
        }
        let from = match &ext {
            TodoExt::Local(l) => l.from.clone().or_else(|| l.notes_line_id.map(|_| "Notes".into())),
            TodoExt::Jira(_) => None,
            TodoExt::Obsidian(_) => t.summary,
        };
        let deadline = t.due.map(|d| d.deadline(clock));
        out.push(Todo {
            item_id: t.item_id,
            title: t.title,
            from,
            due_label: t.due.map(|d| due_label(d, clock)),
            overdue: deadline.is_some_and(|d| d < now),
            due: deadline,
            topic: t.topic,
            rank: None,
            sched_label: t.sched.map(|d| due_label(crate::time::Due::Day(d), clock)),
            sched: t.sched,
            url: t.url,
            estimate: t.estimate.map(|(minutes, by)| Estimate { minutes, by }),
            ext,
        });
    }
    Ok(out)
}

/// Whether a Jira key's project is one of `skip`.
pub(crate) fn project_excluded(key: &str, skip: &[String]) -> bool {
    key.split_once('-').is_some_and(|(p, _)| skip.iter().any(|s| s.eq_ignore_ascii_case(p)))
}

/// Jira projects whose issues aren't todos (`todo_exclude_projects`).
pub(crate) fn excluded_projects(r: &dyn Reads) -> Result<Vec<String>> {
    Ok(r.sources()
        .of_kind("jira")?
        .iter()
        .filter_map(|s| serde_json::from_str::<Value>(&s.config_json).ok())
        .flat_map(|c| c.get("todo_exclude_projects").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect())
}

/// The enabled Jira source new Jira todos go to.
fn jira_source(r: &dyn Reads) -> Result<Option<SourceRow>> {
    Ok(r.sources().of_kind("jira")?.into_iter().find(|s| s.enabled))
}

fn jira_target(r: &dyn Reads) -> Result<Option<Value>> {
    let cfg: Value = jira_source(r)?.and_then(|s| serde_json::from_str(&s.config_json).ok()).unwrap_or(Value::Null);
    Ok(cfg.get("create_project").and_then(Value::as_str).map(|p| {
        json!({ "project": p, "issue_type": cfg.get("create_issue_type").and_then(Value::as_str).unwrap_or("Task") })
    }))
}

pub fn complete(uow: &dyn UnitOfWork, item_id: i64, done: bool) -> Result<()> {
    let item = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?;
    let status = if done { "done" } else { "open" };
    match TodoExt::parse(&item.source_kind, serde_json::from_str(&item.raw_json).unwrap_or_default()).ok_or_else(|| msg("not a todo"))? {
        TodoExt::Local(_) => uow.items().set_status(item_id, status)?,
        TodoExt::Jira(j) => {
            let to = if done { "Done".to_string() } else if j.status.eq_ignore_ascii_case("done") { "To Do".into() } else { j.status };
            request(uow, item.source_id, "jira.transition", &json!({ "key": j.key, "transition_name": to, "done": done }), Some(item_id))?;
        }
        TodoExt::Obsidian(o) => {
            request(uow, item.source_id, "obsidian.toggle_task", &json!({ "path": o.path, "task_text": o.text, "done": done }), Some(item_id))?;
        }
    }
    Ok(())
}

/// A local todo's scheduled day (a soft date: Priorities counts it from that
/// day on). Jira and Obsidian todos have none.
pub fn schedule(uow: &dyn UnitOfWork, item_id: i64, day: Option<Date>) -> Result<()> {
    let item = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?;
    match TodoExt::parse(&item.source_kind, serde_json::from_str(&item.raw_json).unwrap_or_default()) {
        Some(TodoExt::Local(_)) => uow.items().set_sched_day(item_id, day),
        Some(_) => Err(msg("only local todos have a scheduled date")),
        None => Err(msg("not a todo")),
    }
}

pub fn set_estimate(uow: &dyn UnitOfWork, item_id: i64, minutes: Option<i64>) -> Result<()> {
    if minutes.is_some_and(|m| m <= 0) {
        return Err(msg("an estimate is a positive number of minutes"));
    }
    uow.todos().set_estimate(item_id, minutes)
}

pub struct Draft {
    pub target: String,
    pub text: String,
    pub due: Option<String>,
    /// The scheduled day, local todos only.
    pub sched: Option<String>,
}

impl Draft {
    fn from_args(args: &Value) -> Result<Draft> {
        let target = arg_str(args, "target")?;
        if target != "local" && target != "jira" {
            return Err(msg("a todo goes to 'local' or 'jira'"));
        }
        let text = arg_str(args, "text")?.trim().to_string();
        if text.is_empty() {
            return Err(msg("a todo needs its text"));
        }
        let date = |k: &str| -> Result<Option<String>> {
            let v = args.get(k).and_then(Value::as_str).map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned);
            if let Some(d) = &v {
                parse_date(d)?;
            }
            Ok(v)
        };
        let (due, sched) = (date("due")?, date("sched")?);
        if sched.is_some() && target != "local" {
            return Err(msg("only local todos have a scheduled date"));
        }
        Ok(Draft { target, text, due, sched })
    }
}

pub fn create(uow: &dyn UnitOfWork, now: Ts, d: &Draft, topic_id: Option<i64>, from_item: Option<i64>) -> Result<()> {
    if d.target == "jira" {
        let jira = jira_source(uow.reads())?.ok_or_else(|| msg("no Jira source to create the issue in"))?;
        request(uow, jira.id, "jira.create_issue", &json!({ "summary": d.text, "assign_me": true, "due": d.due, "topic_id": topic_id }), None)?;
        return Ok(());
    }
    let (from, url) = match from_item.map(|id| uow.items().get(id)).transpose()?.flatten() {
        Some(i) => (Some(i.title), i.url),
        None => (None, None),
    };
    let src = uow.sources().ensure_local("notes", super::notes::NOTES_SOURCE)?;
    let ext = LocalExt { from, from_item, notes_line_id: None };
    let id = uow.items().insert(&NewItem {
        source_id: src,
        external_id: "todo-new".into(),
        kind: "task".into(),
        title: d.text.clone(),
        url,
        occurred_at: now,
        due_day: d.due.as_deref().map(parse_date).transpose()?,
        sched_day: d.sched.as_deref().map(parse_date).transpose()?,
        status: "open".into(),
        owner: Some("you".into()),
        seen: true,
        raw_json: serde_json::to_string(&ext)?,
        ..Default::default()
    })?;
    uow.items().set_external_id(id, &format!("todo-{id}"))?;
    if let Some(t) = topic_id {
        uow.links().add_confirmed(id, t, "made as a todo")?;
    }
    Ok(())
}

/// A todo held on a signal is made when the signal is filed under `topic_id`.
pub fn settle(uow: &dyn UnitOfWork, now: Ts, item_id: i64, topic_id: i64) -> Result<bool> {
    let Some(h) = uow.todos().held(item_id)? else { return Ok(false) };
    create(uow, now, &Draft { target: h.target, text: h.text, due: h.due, sched: h.sched }, Some(topic_id), Some(item_id))?;
    uow.todos().unhold(item_id)?;
    Ok(true)
}

pub use crate::domain::todos::Held;

/// Jira confirmed an issue made as your todo: it's a todo now, before the next poll.
pub fn issue_created(uow: &dyn UnitOfWork, now: Ts, payload: &Value, receipt: &Value) -> Result<()> {
    let (Some(key), Some(url)) = (receipt.get("key").and_then(Value::as_str), receipt.get("url").and_then(Value::as_str)) else {
        return Ok(());
    };
    let Some(jira) = jira_source(uow.reads())? else { return Ok(()) };
    let summary = payload.get("summary").and_then(Value::as_str).unwrap_or_default();
    let due = payload.get("due").and_then(Value::as_str).and_then(|d| d.parse::<Date>().ok());
    let raw = json!({ "key": key, "fields": { "summary": summary, "status": { "name": "To Do" } } });
    let id = uow.items().insert_or_get(&NewItem {
        source_id: jira.id,
        external_id: key.into(),
        kind: "issue".into(),
        title: format!("{key} · {summary}"),
        url: Some(url.into()),
        occurred_at: now,
        due_day: due,
        status: "open".into(),
        owner: Some("you".into()),
        seen: true,
        raw_json: raw.to_string(),
        ..Default::default()
    })?;
    if let Some(topic) = payload.get("topic_id").and_then(Value::as_i64) {
        uow.links().add_confirmed(id, topic, "made as a todo")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::App;
    use crate::time::Due;
    use rusqlite::{params, Connection};

    fn held(conn: &Connection, item: i64) -> Vec<(i64, Held)> {
        Reads::todos(&crate::store::over(conn)).held(&[item]).unwrap()
    }

    fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
        crate::api::call(app, &format!("todos.{name}"), args)
    }
    use crate::settings::Settings;

    /// Confirm files the signal and makes its held todo in one unit of work:
    /// a todo that can't be made leaves the signal exactly as it was.
    #[test]
    fn a_confirm_that_fails_partway_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        let (item, topic): (i64, i64) = app
            .db()
            .query_row(
                "SELECT tl.item_id, tl.topic_id FROM topic_links tl JOIN items i ON i.id = tl.item_id
                  WHERE tl.state = 'suggested' AND i.seen = 0 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        call(&app, "hold", &json!({ "item_id": item, "target": "jira", "text": "File the ticket" })).unwrap();
        app.db().execute("UPDATE sources SET enabled = 0 WHERE kind = 'jira'", []).unwrap();
        let state = || -> (String, bool, i64) {
            app.db()
                .query_row(
                    "SELECT tl.state, i.seen, (SELECT count(*) FROM held_todos WHERE item_id = i.id)
                       FROM topic_links tl JOIN items i ON i.id = tl.item_id WHERE tl.item_id = ?1 AND tl.topic_id = ?2",
                    [item, topic],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        let before = state();
        let err = crate::api::call(&app, "incoming.confirm", &json!({ "item_id": item, "topic_id": topic })).unwrap_err();
        assert!(err.to_string().contains("no Jira source"), "{err}");
        assert_eq!(state(), before, "link, seen and the held todo all as they were");
        assert_eq!(before, ("suggested".to_string(), false, 1));
    }

    #[test]
    fn held_todos_are_made_on_confirm_and_complete_where_they_live() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        let conn = app.db();
        conn.execute("UPDATE sources SET config_json = '{\"create_project\": \"PLAT\"}' WHERE kind = 'jira'", []).unwrap();
        let (item, topic, _note): (i64, i64, String) = conn
            .query_row(
                "SELECT tl.item_id, tl.topic_id, t.obsidian_path FROM topic_links tl JOIN items i ON i.id = tl.item_id
                   JOIN sources s ON s.id = i.source_id JOIN topics t ON t.id = tl.topic_id
                  WHERE s.kind = 'slack' AND tl.state = 'suggested' AND t.obsidian_path IS NOT NULL LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let other: i64 = conn.query_row("SELECT id FROM topics WHERE id != ?1 LIMIT 1", [topic], |r| r.get(0)).unwrap();
        let now = app.clock.clone();
        let before = list(&crate::store::over(&conn), &now).unwrap().len();
        let hold = |target: &str, text: &str| call(&app, "hold", &json!({ "item_id": item, "target": target, "text": text, "due": "2026-10-02" })).unwrap();
        drop(conn);
        assert!(call(&app, "hold", &json!({ "item_id": item, "target": "marvin", "text": "x" })).is_err());
        hold("local", "Reply to Kim");
        crate::queries::incoming::dismiss(&crate::store::over(&app.db()), item, true).unwrap();
        assert!(held(&app.db(), item).is_empty());
        crate::queries::incoming::dismiss(&crate::store::over(&app.db()), item, false).unwrap();
        crate::queries::incoming::move_to(&crate::store::over(&app.db()), crate::time::Ts::new(jiff::Timestamp::now()), item, other, Some(topic)).unwrap();
        let conn = app.db();
        assert!(held(&conn, item).is_empty());
        let todos = list(&crate::store::over(&conn), &now).unwrap();
        assert_eq!(todos.len(), before + 1);
        let t = todos.iter().find(|t| t.title == "Reply to Kim").unwrap();
        // Due on that day: its deadline is the end of Oct 2 here.
        assert_eq!((t.topic.as_ref().map(|t| t.id), t.due), (Some(other), Some(Due::Day(jiff::civil::date(2026, 10, 2)).deadline(&app.clock))));
        assert!(matches!(&t.ext, TodoExt::Local(l) if l.from_item == Some(item) && l.from.is_some()));
        complete(&crate::store::over(&conn), t.item_id, true).unwrap();
        assert_eq!(list(&crate::store::over(&conn), &now).unwrap().len(), before);

        drop(conn);
        hold("jira", "Retry budget for ingest");
        conn_confirm(&app, item, topic);
        let conn = app.db();
        let payload: Value = conn
            .query_row("SELECT payload_json FROM outbox WHERE op = 'jira.create_issue'", [], |r| r.get::<_, String>(0))
            .map(|p| serde_json::from_str(&p).unwrap())
            .unwrap();
        assert_eq!(payload, json!({ "summary": "Retry budget for ingest", "assign_me": true, "due": "2026-10-02", "topic_id": topic }));
        crate::outbox::tests_on_confirmed(&conn, "jira.create_issue", &payload, &json!({ "key": "PLAT-77", "url": "https://j/browse/PLAT-77" }));
        let t = list(&crate::store::over(&conn), &now).unwrap().into_iter().find(|t| matches!(&t.ext, TodoExt::Jira(j) if j.key == "PLAT-77")).unwrap();
        assert_eq!(t.topic.map(|t| t.id), Some(topic));
        let appended: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op LIKE 'obsidian.%'", [], |r| r.get(0)).unwrap();
        assert_eq!(appended, 0, "making a todo never writes to the topic's note");

        complete(&crate::store::over(&conn), t.item_id, true).unwrap();
        let tr: String = conn.query_row("SELECT payload_json FROM outbox WHERE op = 'jira.transition'", [], |r| r.get(0)).unwrap();
        let tr: Value = serde_json::from_str(&tr).unwrap();
        assert_eq!(tr, json!({ "key": "PLAT-77", "transition_name": "Done", "done": true }));
        assert!(list(&crate::store::over(&conn), &now).unwrap().iter().any(|x| x.item_id == t.item_id), "open until Jira confirms");
        crate::outbox::tests_on_confirmed_item(&conn, "jira.transition", t.item_id, &tr);
        assert!(!list(&crate::store::over(&conn), &now).unwrap().iter().any(|x| x.item_id == t.item_id));
    }

    fn conn_confirm(app: &App, item: i64, topic: i64) {
        let conn = app.db();
        conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [item]).unwrap();
        conn.execute("UPDATE topic_links SET state = 'suggested' WHERE item_id = ?1 AND topic_id = ?2", [item, topic]).unwrap();
        crate::queries::incoming::confirm(&crate::store::over(&conn), crate::time::Ts::new(jiff::Timestamp::now()), item, topic).unwrap();
    }

    #[test]
    fn excluded_jira_projects_are_not_todos() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        let conn = app.db();
        for key in ["DPR-7", "DEVOPS-9"] {
            let raw = json!({ "key": key, "fields": { "status": { "name": "To Do" } } }).to_string();
            conn.execute(
                "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, owner, raw_json)
                 SELECT id, ?1, 'issue', ?1, '2026-09-28T09:00:00', 'open', 'you', ?2 FROM sources WHERE kind = 'jira'",
                params![key, raw],
            )
            .unwrap();
        }
        conn.execute("UPDATE sources SET config_json = '{\"todo_exclude_projects\": [\"dpr\"]}' WHERE kind = 'jira'", []).unwrap();
        let keys: Vec<String> =
            list(&crate::store::over(&conn), &app.clock).unwrap().into_iter().filter_map(|t| if let TodoExt::Jira(j) = t.ext { Some(j.key) } else { None }).collect();
        assert!(keys.contains(&"DEVOPS-9".to_string()) && !keys.contains(&"DPR-7".to_string()), "{keys:?}");
    }

    #[test]
    fn a_row_that_does_not_fit_its_source_is_not_a_todo() {
        assert!(TodoExt::parse("jira", json!({ "key": "X-1" })).is_none());
        assert!(TodoExt::parse("obsidian", json!({ "path": "a.md" })).is_none());
        assert_eq!(TodoExt::parse("notes", json!({ "notes_line_id": 4 })), Some(TodoExt::Local(LocalExt { from: None, from_item: None, notes_line_id: Some(4) })));
    }
}

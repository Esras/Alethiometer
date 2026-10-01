//! Todos (22a, 22c): items you own in a system that holds todos; see docs/todos.md.

use super::{arg_i64, arg_str, due_label, opt_i64, parse_date, parse_ts, TopicRef};
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::state::App;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
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
    fn parse(source: &str, raw: Value) -> Option<TodoExt> {
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
    pub due: Option<String>,
    pub due_label: Option<String>,
    pub overdue: bool,
    pub topic: Option<TopicRef>,
    pub priority: Option<String>,
    pub url: Option<String>,
    pub estimate: Option<Estimate>,
    pub ext: TodoExt,
}

const TODO_SQL: &str = "FROM items i JOIN sources s ON s.id = i.source_id LEFT JOIN todo_estimates e ON e.item_id = i.id
     WHERE s.tag = 'work' AND i.owner = 'you' AND i.status = 'open'
       AND ((s.kind IN ('notes', 'obsidian') AND i.kind = 'task') OR (s.kind = 'jira' AND i.kind = 'issue'))";

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let conn = app.db();
    match name {
        "list" => Ok(json!({ "todos": list(&conn, app.clock.now())?, "jira": jira_target(&conn)? })),
        "complete" | "reopen" => {
            complete(&conn, arg_i64(args, "item_id")?, name == "complete")?;
            crate::outbox::wake();
            Ok(json!({ "ok": true }))
        }
        "create" => {
            let d = Draft::from_args(args)?;
            create(&conn, &d, opt_i64(args, "topic_id"), None)?;
            crate::outbox::wake();
            Ok(json!({ "ok": true }))
        }
        "hold" => {
            let d = Draft::from_args(args)?;
            conn.execute(
                "INSERT INTO held_todos (item_id, target, text, due) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (item_id) DO UPDATE SET target = excluded.target, text = excluded.text, due = excluded.due, state = 'held'",
                params![arg_i64(args, "item_id")?, d.target, d.text, d.due],
            )?;
            Ok(json!({ "ok": true }))
        }
        "estimate" => {
            set_estimate(&conn, arg_i64(args, "item_id")?, opt_i64(args, "minutes"))?;
            Ok(json!({ "ok": true }))
        }
        "unhold" => {
            conn.execute("DELETE FROM held_todos WHERE item_id = ?1", [arg_i64(args, "item_id")?])?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(unknown(name)),
    }
}

type Row = (i64, String, String, Option<String>, Option<String>, String, Option<String>, Option<i64>, Option<String>, Option<i64>, Option<String>);

pub fn list(conn: &Connection, now: NaiveDateTime) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT i.id, s.kind, i.title, i.due_at, i.url, i.raw_json, i.summary,
                (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong'
                  ORDER BY tl.state = 'confirmed' DESC LIMIT 1),
                (SELECT quadrant FROM priorities p WHERE p.item_id = i.id),
                e.minutes, e.by
         {TODO_SQL}
         ORDER BY i.due_at IS NULL, i.due_at, i.id"
    ))?;
    let rows: Vec<Row> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let skip = excluded_projects(conn)?;
    let mut out = vec![];
    for (item_id, source, title, due, url, raw, summary, topic, priority, minutes, by) in rows {
        let Some(ext) = TodoExt::parse(&source, serde_json::from_str(&raw).unwrap_or_default()) else { continue };
        if matches!(&ext, TodoExt::Jira(j) if j.key.split_once('-').is_some_and(|(p, _)| skip.iter().any(|s| s.eq_ignore_ascii_case(p)))) {
            continue;
        }
        let from = match &ext {
            TodoExt::Local(l) => l.from.clone().or_else(|| l.notes_line_id.map(|_| "Notes".into())),
            TodoExt::Jira(_) => None,
            TodoExt::Obsidian(_) => summary,
        };
        let at = due.as_deref().and_then(parse_ts);
        out.push(Todo {
            item_id,
            title,
            from,
            due_label: at.map(|t| due_label(t, now)),
            overdue: at.is_some_and(|t| t < now),
            due,
            topic: topic.map(|id| super::topic_ref(conn, id)).transpose()?,
            priority,
            url,
            estimate: minutes.zip(by).map(|(minutes, by)| Estimate { minutes, by }),
            ext,
        });
    }
    Ok(out)
}

fn excluded_projects(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT config_json FROM sources WHERE kind = 'jira'")?;
    let cfgs = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(cfgs
        .iter()
        .filter_map(|c| serde_json::from_str::<Value>(c).ok())
        .flat_map(|c| c.get("todo_exclude_projects").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect())
}

fn jira_target(conn: &Connection) -> Result<Option<Value>> {
    let cfg: Option<String> = conn.query_row("SELECT config_json FROM sources WHERE kind = 'jira' AND enabled = 1", [], |r| r.get(0)).optional()?;
    let cfg: Value = cfg.and_then(|c| serde_json::from_str(&c).ok()).unwrap_or(Value::Null);
    Ok(cfg.get("create_project").and_then(Value::as_str).map(|p| {
        json!({ "project": p, "issue_type": cfg.get("create_issue_type").and_then(Value::as_str).unwrap_or("Task") })
    }))
}

pub fn complete(conn: &Connection, item_id: i64, done: bool) -> Result<()> {
    let (source_id, source, raw): (i64, String, String) = conn
        .query_row("SELECT s.id, s.kind, i.raw_json FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1", [item_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or_else(|| msg("no such item"))?;
    let status = if done { "done" } else { "open" };
    match TodoExt::parse(&source, serde_json::from_str(&raw).unwrap_or_default()).ok_or_else(|| msg("not a todo"))? {
        TodoExt::Local(_) => {
            conn.execute("UPDATE items SET status = ?2 WHERE id = ?1", params![item_id, status])?;
        }
        TodoExt::Jira(j) => {
            let to = if done { "Done".to_string() } else if j.status.eq_ignore_ascii_case("done") { "To Do".into() } else { j.status };
            crate::outbox::enqueue(conn, source_id, "jira.transition", &json!({ "key": j.key, "transition_name": to, "done": done }), Some(item_id))?;
        }
        TodoExt::Obsidian(o) => {
            crate::outbox::enqueue(conn, source_id, "obsidian.toggle_task", &json!({ "path": o.path, "task_text": o.text, "done": done }), Some(item_id))?;
        }
    }
    Ok(())
}

pub fn set_estimate(conn: &Connection, item_id: i64, minutes: Option<i64>) -> Result<()> {
    match minutes {
        Some(m) if m > 0 => conn.execute(
            "INSERT INTO todo_estimates (item_id, minutes, by) VALUES (?1, ?2, 'you')
             ON CONFLICT (item_id) DO UPDATE SET minutes = excluded.minutes, by = 'you', updated_at = datetime('now', 'localtime')",
            params![item_id, m],
        )?,
        Some(_) => return Err(msg("an estimate is a positive number of minutes")),
        None => conn.execute("DELETE FROM todo_estimates WHERE item_id = ?1", [item_id])?,
    };
    Ok(())
}

pub struct Draft {
    pub target: String,
    pub text: String,
    pub due: Option<String>,
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
        let due = args.get("due").and_then(Value::as_str).map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned);
        if let Some(d) = &due {
            parse_date(d)?;
        }
        Ok(Draft { target, text, due })
    }
}

pub fn create(conn: &Connection, d: &Draft, topic_id: Option<i64>, from_item: Option<i64>) -> Result<()> {
    if d.target == "jira" {
        let jira: i64 = conn
            .query_row("SELECT id FROM sources WHERE kind = 'jira' AND enabled = 1", [], |r| r.get(0))
            .optional()?
            .ok_or_else(|| msg("no Jira source to create the issue in"))?;
        crate::outbox::enqueue(conn, jira, "jira.create_issue", &json!({ "summary": d.text, "assign_me": true, "due": d.due, "topic_id": topic_id }), None)?;
        return Ok(());
    }
    let (from, url): (Option<String>, Option<String>) = match from_item {
        Some(id) => conn.query_row("SELECT title, url FROM items WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.unwrap_or_default(),
        None => (None, None),
    };
    let src = super::notes::alethiometer_source(conn)?;
    let ext = LocalExt { from, from_item, notes_line_id: None };
    conn.execute(
        "INSERT INTO items (source_id, external_id, kind, title, url, occurred_at, due_at, status, owner, seen, raw_json)
         VALUES (?1, 'todo-new', 'task', ?2, ?3, strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'), ?4, 'open', 'you', 1, ?5)",
        params![src, d.text, url, d.due.as_ref().map(|d| format!("{d}T{}", super::DAY_ONLY_DUE)), serde_json::to_string(&ext)?],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute("UPDATE items SET external_id = 'todo-' || id WHERE id = ?1", [id])?;
    if let Some(t) = topic_id {
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'confirmed', 1.0, 'made as a todo')", params![id, t])?;
    }
    Ok(())
}

pub fn settle(conn: &Connection, item_id: i64, topic_id: i64) -> Result<bool> {
    let held: Option<(String, String, Option<String>)> = conn
        .query_row("SELECT target, text, due FROM held_todos WHERE item_id = ?1 AND state = 'held'", [item_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((target, text, due)) = held else { return Ok(false) };
    create(conn, &Draft { target, text, due }, Some(topic_id), Some(item_id))?;
    conn.execute("DELETE FROM held_todos WHERE item_id = ?1", [item_id])?;
    Ok(true)
}

pub fn discard(conn: &Connection, item_id: i64, on: bool) -> Result<()> {
    conn.execute("UPDATE held_todos SET state = ?2 WHERE item_id = ?1", params![item_id, if on { "discarded" } else { "held" }])?;
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
pub struct Held {
    pub target: String,
    pub text: String,
    pub due: Option<String>,
}

pub fn held(conn: &Connection, item_id: i64) -> Result<Option<Held>> {
    Ok(conn
        .query_row("SELECT target, text, due FROM held_todos WHERE item_id = ?1 AND state = 'held'", [item_id], |r| {
            Ok(Held { target: r.get(0)?, text: r.get(1)?, due: r.get(2)? })
        })
        .optional()?)
}

pub fn issue_created(conn: &Connection, payload: &Value, receipt: &Value) -> Result<()> {
    let (Some(key), Some(url)) = (receipt.get("key").and_then(Value::as_str), receipt.get("url").and_then(Value::as_str)) else {
        return Ok(());
    };
    let summary = payload.get("summary").and_then(Value::as_str).unwrap_or_default();
    let due = payload.get("due").and_then(Value::as_str).map(|d| format!("{d}T{}", super::DAY_ONLY_DUE));
    let raw = json!({ "key": key, "fields": { "summary": summary, "status": { "name": "To Do" } } });
    conn.execute(
        "INSERT INTO items (source_id, external_id, kind, title, url, occurred_at, due_at, status, owner, seen, raw_json)
         SELECT id, ?1, 'issue', ?2, ?3, strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'), ?4, 'open', 'you', 1, ?5
           FROM sources WHERE kind = 'jira' AND enabled = 1
         ON CONFLICT (source_id, external_id) DO NOTHING",
        params![key, format!("{key} · {summary}"), url, due, raw.to_string()],
    )?;
    let Some(topic) = payload.get("topic_id").and_then(Value::as_i64) else { return Ok(()) };
    conn.execute(
        "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
         SELECT i.id, ?2, 'confirmed', 1.0, 'made as a todo' FROM items i JOIN sources s ON s.id = i.source_id
          WHERE s.kind = 'jira' AND i.external_id = ?1
         ON CONFLICT (item_id, topic_id) DO NOTHING",
        params![key, topic],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

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
        let now = app.clock.now();
        let before = list(&conn, now).unwrap().len();
        let hold = |target: &str, text: &str| call(&app, "hold", &json!({ "item_id": item, "target": target, "text": text, "due": "2026-10-02" })).unwrap();
        drop(conn);
        assert!(call(&app, "hold", &json!({ "item_id": item, "target": "marvin", "text": "x" })).is_err());
        hold("local", "Reply to Kim");
        crate::queries::incoming::dismiss(&app.db(), item, true).unwrap();
        assert!(held(&app.db(), item).unwrap().is_none());
        crate::queries::incoming::dismiss(&app.db(), item, false).unwrap();
        crate::queries::incoming::move_to(&app.db(), item, other, Some(topic)).unwrap();
        let conn = app.db();
        assert!(held(&conn, item).unwrap().is_none());
        let todos = list(&conn, now).unwrap();
        assert_eq!(todos.len(), before + 1);
        let t = todos.iter().find(|t| t.title == "Reply to Kim").unwrap();
        assert_eq!((t.topic.as_ref().map(|t| t.id), t.due.as_deref()), (Some(other), Some("2026-10-02T23:59:59")));
        assert!(matches!(&t.ext, TodoExt::Local(l) if l.from_item == Some(item) && l.from.is_some()));
        complete(&conn, t.item_id, true).unwrap();
        assert_eq!(list(&conn, now).unwrap().len(), before);

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
        let t = list(&conn, now).unwrap().into_iter().find(|t| matches!(&t.ext, TodoExt::Jira(j) if j.key == "PLAT-77")).unwrap();
        assert_eq!(t.topic.map(|t| t.id), Some(topic));
        let appended: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op LIKE 'obsidian.%'", [], |r| r.get(0)).unwrap();
        assert_eq!(appended, 0, "making a todo never writes to the topic's note");

        complete(&conn, t.item_id, true).unwrap();
        let tr: String = conn.query_row("SELECT payload_json FROM outbox WHERE op = 'jira.transition'", [], |r| r.get(0)).unwrap();
        let tr: Value = serde_json::from_str(&tr).unwrap();
        assert_eq!(tr, json!({ "key": "PLAT-77", "transition_name": "Done", "done": true }));
        assert!(list(&conn, now).unwrap().iter().any(|x| x.item_id == t.item_id), "open until Jira confirms");
        crate::outbox::tests_on_confirmed_item(&conn, "jira.transition", t.item_id, &tr);
        assert!(!list(&conn, now).unwrap().iter().any(|x| x.item_id == t.item_id));
    }

    fn conn_confirm(app: &App, item: i64, topic: i64) {
        let conn = app.db();
        conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [item]).unwrap();
        conn.execute("UPDATE topic_links SET state = 'suggested' WHERE item_id = ?1 AND topic_id = ?2", [item, topic]).unwrap();
        crate::queries::incoming::confirm(&conn, item, topic).unwrap();
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
            list(&conn, app.clock.now()).unwrap().into_iter().filter_map(|t| if let TodoExt::Jira(j) = t.ext { Some(j.key) } else { None }).collect();
        assert!(keys.contains(&"DEVOPS-9".to_string()) && !keys.contains(&"DPR-7".to_string()), "{keys:?}");
    }

    #[test]
    fn a_row_that_does_not_fit_its_source_is_not_a_todo() {
        assert!(TodoExt::parse("jira", json!({ "key": "X-1" })).is_none());
        assert!(TodoExt::parse("obsidian", json!({ "path": "a.md" })).is_none());
        assert_eq!(TodoExt::parse("notes", json!({ "notes_line_id": 4 })), Some(TodoExt::Local(LocalExt { from: None, from_item: None, notes_line_id: Some(4) })));
    }
}

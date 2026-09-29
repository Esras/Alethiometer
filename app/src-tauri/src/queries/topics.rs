//! Topics (7a): the root object. Upcoming, Open, Chronology, People, and the
//! topic's Obsidian note.

use super::{arg_i64, arg_str, due_is_day_only, due_label, dur_label, parse_ts, when_label, Pill};
use crate::api::unknown;
use crate::error::Result;
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use crate::sync::connectors::obsidian::save;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct TopicRow {
    id: i64,
    name: String,
    category: String,
    obsidian_path: Option<String>,
    new_count: i64,
    open_count: i64,
    last_signal_at: Option<String>,
    last_signal: Option<String>,
    quiet: bool,
    next_meeting: Option<Meeting>,
}

#[derive(Serialize)]
struct Meeting {
    start: String,
    title: String,
}

#[derive(Serialize)]
struct Upcoming {
    item_id: i64,
    when: String,
    today: bool,
    source: String,
    title: String,
    sub: Option<String>,
}

#[derive(Serialize)]
struct OpenItem {
    item_id: i64,
    source: String,
    title: String,
    due: Option<Pill>,
    owner: Option<String>,
}

#[derive(Serialize)]
struct Entry {
    item_id: i64,
    when: String,
    today: bool,
    source: String,
    title: String,
    /// Time range for threads ("09:10–11:40").
    range: Option<String>,
    sub: Option<String>,
    right: String,
    /// Open dot: upcoming or not done. Solid dot: happened.
    open: bool,
}

#[derive(Serialize)]
struct Person {
    id: i64,
    name: String,
    role: Option<String>,
    initials: String,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list" => list(app, args.get("archived").and_then(Value::as_bool).unwrap_or(false)),
        "set_category" => {
            let (id, cat) = (arg_i64(args, "id")?, arg_str(args, "category")?);
            let conn = app.db();
            if !super::categories::exists(&conn, &cat)? {
                return Err(crate::error::msg(format!("unknown category '{cat}'")));
            }
            conn.execute("UPDATE topics SET category = ?2 WHERE id = ?1", params![id, cat])?;
            Ok(json!({ "ok": true }))
        }
        "archive" => set_archived(app, arg_i64(args, "id")?, true),
        "unarchive" => set_archived(app, arg_i64(args, "id")?, false),
        "get" => get(app, arg_i64(args, "id")?),
        "create" => create(app, &arg_str(args, "name")?, args.get("category").and_then(Value::as_str)),
        "save_note" => save_note(app, arg_i64(args, "id")?, &arg_str(args, "text")?, args.get("base").and_then(Value::as_str)),
        "note_status" => save::status(app, arg_i64(args, "outbox_id")?),
        "keep_mine" => save::keep_mine(app, arg_i64(args, "outbox_id")?),
        _ => Err(unknown(name)),
    }
}

fn create(app: &App, name: &str, category: Option<&str>) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::error::msg("Topic name can't be empty"));
    }
    let category = category.unwrap_or("work");
    let conn = app.db();
    if !super::categories::exists(&conn, category)? {
        return Err(crate::error::msg(format!("unknown category '{category}'")));
    }
    if find_by_name(&conn, name)?.is_some() {
        return Err(crate::error::msg(format!("a topic named '{name}' already exists")));
    }
    Ok(json!({ "id": find_or_create(&conn, name, category)? }))
}

pub(crate) fn find_by_name(conn: &rusqlite::Connection, name: &str) -> Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    Ok(conn.query_row("SELECT id FROM topics WHERE archived_at IS NULL AND name = ?1 COLLATE NOCASE", [name], |r| r.get(0)).optional()?)
}

pub(crate) fn find_or_create(conn: &rusqlite::Connection, name: &str, category: &str) -> Result<i64> {
    if let Some(id) = find_by_name(conn, name)? {
        return Ok(id);
    }
    conn.execute("INSERT INTO topics (name, category) VALUES (?1, ?2)", params![name, category])?;
    Ok(conn.last_insert_rowid())
}

/// The topic's note edited in place (Topics pane, `e`). `base` is the text the
/// editor loaded (`note.base` from `get`; null when the note didn't exist).
/// Answers `{state: "saved"}`, `{state: "conflict", disk}` or
/// `{state: "saving", outbox_id}` (follow it with `note_status`).
fn save_note(app: &App, id: i64, text: &str, base: Option<&str>) -> Result<Value> {
    let path: Option<String> = app.db().query_row("SELECT obsidian_path FROM topics WHERE id = ?1", [id], |r| r.get(0))?;
    let path = path.ok_or_else(|| crate::error::msg("this topic has no Obsidian note"))?;
    let rel = path.trim_start_matches("./").trim_start_matches('/');
    Ok(match save::save_note(app, rel, base, text)? {
        save::Saved::Written => json!({ "state": "saved" }),
        save::Saved::Conflict(disk) => json!({ "state": "conflict", "disk": disk }),
        save::Saved::Queued(outbox_id) => json!({ "state": "saving", "outbox_id": outbox_id }),
    })
}

fn list(app: &App, archived: bool) -> Result<Value> {
    let now = app.clock.now();
    let now_s = crate::seed::fmt(now);
    let conn = app.db();
    let links = "topic_links tl JOIN items i ON i.id = tl.item_id";
    let mine = "tl.topic_id = t.id AND tl.state = 'confirmed'";
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.name, t.category, t.obsidian_path,
                (SELECT count(*) FROM {links} WHERE tl.topic_id = t.id AND tl.state IN ('suggested', 'confirmed') AND i.seen = 0),
                (SELECT count(*) FROM {links}
                  WHERE {mine} AND i.kind IN ('task', 'issue', 'email') AND i.owner IS NOT NULL AND i.status = 'open'),
                (SELECT max(coalesce(i.ended_at, i.occurred_at)) FROM {links} WHERE {mine} AND i.occurred_at <= ?1),
                (SELECT s.kind FROM {links} JOIN sources s ON s.id = i.source_id WHERE {mine} AND i.occurred_at <= ?1
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC LIMIT 1),
                (SELECT e.start || char(9) || coalesce(e.title, '') FROM {links} JOIN events e ON e.item_id = i.id
                  WHERE {mine} AND e.start > ?1 AND e.start <= ?2 ORDER BY e.start LIMIT 1)
           FROM topics t WHERE (t.archived_at IS NOT NULL) = ?3"
    ))?;
    let week = crate::seed::fmt(now + chrono::Duration::days(7));
    let quiet_before = crate::seed::fmt(now - chrono::Duration::days(app.settings.quiet_days));
    let mut rows = stmt
        .query_map(params![now_s, week, archived], |r| {
            let last: Option<String> = r.get(6)?;
            let source: Option<String> = r.get(7)?;
            let meeting: Option<String> = r.get(8)?;
            Ok(TopicRow {
                id: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                obsidian_path: r.get(3)?,
                new_count: r.get(4)?,
                open_count: r.get(5)?,
                last_signal: last.as_deref().and_then(parse_ts).map(|t| last_label(t, now, source.as_deref())),
                quiet: last.as_ref().is_none_or(|l| *l < quiet_before),
                last_signal_at: last,
                next_meeting: meeting.and_then(|m| {
                    let (start, title) = m.split_once('\t')?;
                    Some(Meeting { start: start.into(), title: title.into() })
                }),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Active first (next meeting soonest, then most recent signal), then quiet.
    rows.sort_by(|a, b| {
        let meet = |t: &TopicRow| t.next_meeting.as_ref().map(|m| m.start.clone());
        (a.quiet, meet(a).is_none(), meet(a))
            .cmp(&(b.quiet, meet(b).is_none(), meet(b)))
            .then_with(|| b.last_signal_at.cmp(&a.last_signal_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(json!(rows))
}

/// "10:41 · Slack" today, then "Yesterday", a weekday within the week, else "Sep 25".
fn last_label(t: chrono::NaiveDateTime, now: chrono::NaiveDateTime, source: Option<&str>) -> String {
    match (now.date() - t.date()).num_days() {
        0 => match source {
            Some(s) => format!("{} · {}", t.format("%H:%M"), source_label(s)),
            None => t.format("%H:%M").to_string(),
        },
        1 => "Yesterday".into(),
        2..=6 => t.format("%a").to_string(),
        _ => t.format("%b %-d").to_string(),
    }
}

fn set_archived(app: &App, id: i64, on: bool) -> Result<Value> {
    let n = app.db().execute(
        "UPDATE topics SET archived_at = CASE WHEN ?2 THEN datetime('now') END WHERE id = ?1",
        params![id, on],
    )?;
    if n == 0 {
        return Err(crate::error::msg("no such topic"));
    }
    Ok(json!({ "ok": true }))
}

fn source_label(kind: &str) -> &'static str {
    match kind {
        "gcal" => "Google Cal",
        "slack" => "Slack",
        "jira" => "Jira",
        "github" => "GitHub",
        "obsidian" => "Obsidian",
        "gmail" => "Gmail",
        "sheets" => "Sheets",
        "notion" => "Notion",
        "marvin" => "Marvin",
        "protoncal" => "Proton Cal",
        "notes" => "Notes",
        _ => "",
    }
}

fn get(app: &App, id: i64) -> Result<Value> {
    let now = app.clock.now();
    let now_s = crate::seed::fmt(now);
    let conn = app.db();

    let (name, category, path, score, pills): (String, String, Option<String>, Option<f64>, String) = conn.query_row(
        "SELECT name, category, obsidian_path, score, pills_json FROM topics WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let pills: Vec<Pill> = serde_json::from_str(&pills).unwrap_or_default();

    // All non-wrong links; suggestions still waiting in Incoming are left out.
    let mut stmt = conn.prepare(
        "SELECT i.id, s.kind, i.kind, i.title, i.summary, i.occurred_at, i.ended_at, i.due_at, i.status,
                i.msg_count, i.owner, json_extract(i.raw_json, '$.recurringEventId')
           FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
          WHERE tl.topic_id = ?1 AND tl.state = 'confirmed'",
    )?;
    struct Row {
        id: i64,
        source: String,
        kind: String,
        title: String,
        summary: Option<String>,
        at: String,
        ended: Option<String>,
        due: Option<String>,
        status: String,
        msgs: Option<i64>,
        owner: Option<String>,
        series: Option<String>,
    }
    let rows = stmt
        .query_map([id], |r| {
            Ok(Row {
                id: r.get(0)?,
                source: r.get(1)?,
                kind: r.get(2)?,
                title: r.get(3)?,
                summary: r.get(4)?,
                at: r.get(5)?,
                ended: r.get(6)?,
                due: r.get(7)?,
                status: r.get(8)?,
                msgs: r.get(9)?,
                owner: r.get(10)?,
                series: r.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut next_of_series: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for r in rows.iter().filter(|r| r.at > now_s) {
        if let Some(s) = &r.series {
            let e = next_of_series.entry(s).or_insert(&r.at);
            if r.at.as_str() < *e {
                *e = &r.at;
            }
        }
    }

    let mut upcoming = vec![];
    let mut open = vec![];
    let mut chron: Vec<(String, Entry)> = vec![];
    for r in &rows {
        let at = parse_ts(&r.at).unwrap_or(now);
        let is_event = r.kind == "event";
        let mins = r.ended.as_deref().and_then(parse_ts).map(|e| (e - at).num_minutes());
        if is_event && r.at > now_s && (at.date() - now.date()).num_days() <= 7 {
            upcoming.push((r.at.clone(), Upcoming {
                item_id: r.id,
                when: when_label(at, now, true),
                today: at.date() == now.date(),
                source: r.source.clone(),
                title: r.title.clone(),
                sub: match (mins, &r.summary) {
                    (Some(m), Some(s)) => Some(format!("{} · {s}", dur_label(m))),
                    (Some(m), None) => Some(dur_label(m)),
                    (None, s) => s.clone(),
                },
            }));
        }
        let is_task = matches!(r.kind.as_str(), "task" | "issue" | "email") && r.owner.is_some();
        if is_task && r.status == "open" {
            open.push(OpenItem {
                item_id: r.id,
                source: r.source.clone(),
                title: r.title.clone(),
                due: r.due.as_deref().and_then(parse_ts).map(|d| Pill {
                    label: format!("due {}", due_label(d, now)),
                    weight: "yellow".into(),
                }),
                owner: r.owner.clone(),
            });
        }

        // Timeline: open tasks sit at their due date; everything else at when it happened.
        let (t_str, title, sub, right) = if is_task && r.status == "open" {
            match &r.due {
                Some(d) => (d.clone(), r.title.clone(), Some("due".to_string()), source_label(&r.source).to_string()),
                None => continue,
            }
        } else {
            let right = match (mins, r.msgs) {
                (Some(m), _) if is_event => dur_label(m),
                (_, Some(n)) => format!("{n} msgs"),
                _ => source_label(&r.source).to_string(),
            };
            let names = is_event && r.summary.as_deref().map_or(false, is_names);
            let (title, sub) = match &r.summary {
                Some(s) if names => (format!("{} · {s}", r.title), None),
                s => (r.title.clone(), s.clone()),
            };
            (r.at.clone(), title, sub, right)
        };
        let t = parse_ts(&t_str).unwrap_or(now);
        let is_due = is_task && r.status == "open";
        // Events show their time; due dates show one only when they have it.
        let has_time = is_event || (is_due && !due_is_day_only(t));
        let range = if r.kind == "thread" {
            r.ended.as_deref().and_then(parse_ts).map(|e| format!("{}–{}", at.format("%H:%M"), e.format("%H:%M")))
        } else {
            None
        };
        if r.series.as_deref().is_some_and(|s| r.at > now_s && next_of_series.get(s) != Some(&r.at.as_str())) {
            continue;
        }
        chron.push((t_str.clone(), Entry {
            item_id: r.id,
            when: when_label(t, now, has_time),
            // Only today's meetings are lit; due dates stay grey like everything else.
            today: !is_due && t.date() == now.date(),
            source: r.source.clone(),
            title,
            range,
            sub,
            right,
            open: t_str > now_s || (is_task && r.status == "open"),
        }));
    }
    upcoming.sort_by(|a, b| a.0.cmp(&b.0));
    chron.sort_by(|a, b| b.0.cmp(&a.0));

    let mut filters: Vec<(String, i64)> = vec![];
    for (_, e) in &chron {
        match filters.iter_mut().find(|(k, _)| k == &e.source) {
            Some(f) => f.1 += 1,
            None => filters.push((e.source.clone(), 1)),
        }
    }

    let people = conn
        .prepare(
            "SELECT p.id, p.display_name, p.role FROM topic_people tp JOIN people p ON p.id = tp.person_id
              WHERE tp.topic_id = ?1 ORDER BY p.display_name",
        )?
        .query_map([id], |r| {
            let name: String = r.get(1)?;
            Ok(Person { id: r.get(0)?, initials: name.chars().take(2).collect::<String>().to_uppercase(), name, role: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let repos = conn
        .prepare("SELECT repo FROM topic_repos WHERE topic_id = ?1 ORDER BY repo")?
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // `base` is what an edit starts from and must still be there when it saves
    // (null: no file yet). The file on disk is what Obsidian last saved.
    let note = path.as_ref().map(|p| {
        let abs = app.vault.join(p);
        let disk = std::fs::read_to_string(&abs).ok();
        json!({ "path": p, "abs_path": abs.to_string_lossy(), "text": disk.clone().unwrap_or_default(), "base": disk })
    });

    Ok(json!({
        "topic": { "id": id, "name": name, "category": category, "score": score, "pills": pills },
        "upcoming": upcoming.into_iter().map(|(_, u)| u).collect::<Vec<_>>(),
        "open": open,
        "chronology": chron.into_iter().map(|(_, e)| e).collect::<Vec<_>>(),
        "filters": filters.into_iter().map(|(source, count)| json!({ "source": source, "count": count })).collect::<Vec<_>>(),
        "people": people,
        "repos": repos,
        "note": note,
    }))
}

/// Attendee summaries ("Ravi, Kim") read better inline with the title.
fn is_names(s: &str) -> bool {
    !s.is_empty() && s.split(", ").all(|w| w.chars().next().map_or(false, char::is_uppercase) && !w.contains(' '))
}

pub fn seed(ctx: &Ctx, vault: &std::path::Path) -> anyhow::Result<()> {
    let c = ctx.conn;
    let dd = "Datadog renewal";
    let counter_due = ctx.today + chrono::Duration::days(2);
    let pills = serde_json::json!([
        { "label": "$ at stake", "weight": "red" },
        { "label": format!("counter due {}", counter_due.format("%a")), "weight": "yellow" },
        { "label": "prep today", "weight": "green" },
    ]);
    c.execute("UPDATE topics SET score = 5.8, pills_json = ?1 WHERE id = ?2", params![pills.to_string(), ctx.topic(dd)])?;
    for p in ["Kim", "Ravi", "Ana"] {
        c.execute("INSERT INTO topic_people VALUES (?1, ?2)", params![ctx.topic(dd), ctx.person(p)])?;
    }
    c.execute("INSERT INTO topic_repos VALUES (?1, 'platform/observability-config')", [ctx.topic(dd)])?;

    // Today's prep meeting (already on the strip as "DD prep"): give it the long title.
    c.execute(
        "UPDATE items SET title = 'Prep with Kim before Thursday', summary = 'agree walk-away number'
          WHERE id = (SELECT item_id FROM events WHERE title = 'DD prep')",
        [],
    )?;

    let link = |id: i64| ctx.link(id, dd, "confirmed");
    link(ctx.meeting(3, "14:00", "14:45", "Final terms call", "work", "Ravi, Kim", false));
    link(ctx.meeting(-4, "15:00", "15:45", "Negotiation call", "work", "Ravi, Kim", false));
    c.execute(
        "UPDATE items SET summary = 'they offered 12% off for 3 years' WHERE title = 'Negotiation call'",
        [],
    )?;
    link(ctx.meeting(-16, "10:00", "10:30", "Internal usage review", "work", "Ana, Kim", false));
    link(ctx.meeting(-25, "11:00", "11:30", "Kickoff", "work", "Ravi", false));

    let item = |it: ItemSeed| link(ctx.item(it));
    item(ItemSeed {
        source: "obsidian",
        kind: "task",
        title: "Send counter-offer to Ravi".into(),
        occurred_at: ctx.at(-3, "09:00"),
        due_at: Some(ctx.at(2, "17:00")),
        owner: Some("you".into()),
        ..Default::default()
    });
    item(ItemSeed {
        source: "obsidian",
        kind: "task",
        title: "Decide 1y vs 3y".into(),
        occurred_at: ctx.at(-3, "09:00"),
        owner: Some("you".into()),
        ..Default::default()
    });
    item(ItemSeed {
        source: "gmail",
        kind: "email",
        title: "Legal redlines back to vendor".into(),
        occurred_at: ctx.at(-2, "12:00"),
        owner: Some("Kim".into()),
        ..Default::default()
    });
    item(ItemSeed {
        source: "gmail",
        kind: "thread",
        title: "Legal · redlines on MSA §7".into(),
        summary: Some("liability cap raised to 2× fees".into()),
        occurred_at: ctx.at(-2, "09:30"),
        msg_count: Some(3),
        ..Default::default()
    });
    item(ItemSeed {
        source: "jira",
        kind: "issue",
        title: "PROC-412 · renewal approval".into(),
        summary: Some("In review · Finance".into()),
        occurred_at: ctx.at(-2, "08:00"),
        ..Default::default()
    });
    item(ItemSeed {
        source: "slack",
        kind: "thread",
        title: "DM · Kim".into(),
        summary: Some("Procurement prefers 3y; wants your sign-off on the risk".into()),
        occurred_at: ctx.at(-6, "09:10"),
        ended_at: Some(ctx.at(-6, "11:40")),
        msg_count: Some(9),
        ..Default::default()
    });
    item(ItemSeed {
        source: "sheets",
        kind: "sheet",
        title: "Cost model v3".into(),
        summary: Some("Ana edited · 1y vs 3y tabs".into()),
        occurred_at: ctx.at(-10, "16:00"),
        ..Default::default()
    });
    item(ItemSeed {
        source: "notion",
        kind: "doc",
        title: "Renewal requirements".into(),
        summary: Some("30d log retention, SSO, APM seats".into()),
        occurred_at: ctx.at(-12, "11:00"),
        ..Default::default()
    });
    item(ItemSeed {
        source: "slack",
        kind: "thread",
        title: "#finops · \"renewal numbers\"".into(),
        summary: Some("Ana: logs are 18% over committed spend".into()),
        occurred_at: ctx.at(-16, "14:02"),
        ended_at: Some(ctx.at(-16, "15:30")),
        msg_count: Some(23),
        ..Default::default()
    });
    item(ItemSeed {
        source: "sheets",
        kind: "sheet",
        title: "Datadog usage FY26".into(),
        occurred_at: ctx.at(-19, "10:00"),
        ..Default::default()
    });
    item(ItemSeed {
        source: "gmail",
        kind: "thread",
        title: "Ravi · \"Renewal proposal\"".into(),
        occurred_at: ctx.at(-23, "09:00"),
        msg_count: Some(6),
        ..Default::default()
    });

    std::fs::create_dir_all(vault.join("Topics"))?;
    let notes: &[(&str, &str)] = &[
        (
            "Topics/Datadog renewal.md",
            "## Where we are\n- Offer: 12% off for 3y (Sep 24 call)\n- Logs are 18% over commit, so fix retention first\n\n\
             ## My position\n- Prefer 2y + usage true-down clause\n- Won't sign 3y without SSO at no extra cost\n\n\
             ## Asks for Thu\n- [ ] True-down at 12 months\n- [ ] 30d log retention included\n- [ ] Cap on overage rate\n\n\
             ## Links\n- [[Accounts/Datadog]] · [[People/Kim]]\n",
        ),
        ("Topics/Acme outage.md", "## Status\n- Sev 2 since 09:05, retry storm from v2 delivery workers\n- Fix ETA 14:00\n\n## Follow-ups\n- [ ] Postmortem owner\n"),
        ("Topics/Alerting P2.md", "## Noise\n- `disk-pressure node-7` fires every 20 min, no action taken in 3 weeks\n\n## Ask\n- [ ] Jordan to own tuning\n"),
        ("Topics/Q4 headcount.md", "## Options\n- 1 Platform + 1 Data\n- 2 Platform\n\n## Inputs\n- Lee: attrition numbers\n"),
    ];
    for (p, text) in notes {
        let f = vault.join(p);
        if !f.exists() {
            std::fs::write(f, text)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::settings::Settings;
    use crate::state::App;
    use serde_json::json;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        (app, dir)
    }

    fn topic_id(app: &App, name: &str) -> i64 {
        app.db().query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    }

    #[test]
    fn create_dedupes_case_insensitively_and_rejects_a_bad_category() {
        let (app, _dir) = app();
        let r = crate::api::call(&app, "topics.create", &json!({ "name": "New thing" })).unwrap();
        let id = r["id"].as_i64().unwrap();
        assert_eq!(topic_id(&app, "New thing"), id);
        let e = crate::api::call(&app, "topics.create", &json!({ "name": "new thing" })).unwrap_err().to_string();
        assert!(e.contains("already exists"), "{e}");
        let e = crate::api::call(&app, "topics.create", &json!({ "name": "Datadog renewal" })).unwrap_err().to_string();
        assert!(e.contains("already exists"), "{e}");
        let e = crate::api::call(&app, "topics.create", &json!({ "name": "X", "category": "nope" })).unwrap_err().to_string();
        assert!(e.contains("unknown category"), "{e}");
        let e = crate::api::call(&app, "topics.create", &json!({ "name": "  " })).unwrap_err().to_string();
        assert!(e.contains("empty"), "{e}");
    }

    #[test]
    fn index_rows_and_archive() {
        let (app, _dir) = app();
        let list = |archived: bool| crate::api::call(&app, "topics.list", &json!({ "archived": archived })).unwrap();
        let rows = list(false);
        let rows = rows.as_array().unwrap();
        let dd = rows.iter().find(|t| t["name"] == "Datadog renewal").unwrap();
        assert!(dd["open_count"].as_i64().unwrap() >= 3);
        assert!(dd["last_signal"].is_string());
        assert_eq!(dd["obsidian_path"], "Topics/Datadog renewal.md");
        let quiet_at = rows.iter().position(|t| t["quiet"] == true).unwrap_or(rows.len());
        assert!(rows[quiet_at..].iter().all(|t| t["quiet"] == true));
        let id = dd["id"].as_i64().unwrap();
        crate::api::call(&app, "topics.archive", &json!({ "id": id })).unwrap();
        assert!(!list(false).as_array().unwrap().iter().any(|t| t["id"] == id));
        assert!(list(true).as_array().unwrap().iter().any(|t| t["id"] == id));
        crate::api::call(&app, "topics.unarchive", &json!({ "id": id })).unwrap();
        assert!(list(false).as_array().unwrap().iter().any(|t| t["id"] == id));
    }

    #[test]
    fn datadog_topic_has_sections_and_note() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let v = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap();
        assert!(v["open"].as_array().unwrap().len() >= 3);
        assert_eq!(v["people"].as_array().unwrap().len(), 3);
        assert!(v["chronology"].as_array().unwrap().len() >= 14);
        assert!(v["note"]["text"].as_str().unwrap().contains("## Where we are"));
        // Timeline is newest first.
        let whens: Vec<i64> = v["chronology"].as_array().unwrap().iter().map(|e| e["item_id"].as_i64().unwrap()).collect();
        assert!(!whens.is_empty());
    }

    #[test]
    fn edit_the_note_in_place_sample_data() {
        // No obsidian source: the note file is written directly, base-checked.
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let note = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap()["note"].clone();
        let base = note["base"].as_str().unwrap().to_string();
        assert_eq!(note["text"], note["base"]);
        // Reordered sections, a line edited in the middle.
        let mine = base.replace("## Links", "## Links (moved)").replace("Prefer 2y", "Prefer 1y");
        let r = crate::api::call(&app, "topics.save_note", &json!({ "id": id, "text": mine, "base": base })).unwrap();
        assert_eq!(r["state"], "saved");
        let got = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap();
        assert_eq!(got["note"]["text"], mine);
        // A stale base (the note changed in Obsidian meanwhile) writes nothing.
        let r = crate::api::call(&app, "topics.save_note", &json!({ "id": id, "text": "x", "base": base })).unwrap();
        assert_eq!((r["state"].as_str(), r["disk"].as_str()), (Some("conflict"), Some(mine.as_str())));
    }

    #[test]
    fn edit_the_note_in_place_through_obsidian() {
        use crate::sync::http::FakeHttp;
        let (app, _dir) = crate::sync::connectors::obsidian::save::tests::app_with_source("obsidian");
        let id = topic_id(&app, "Datadog renewal");
        let base = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap()["note"]["base"].as_str().unwrap().to_string();
        let mine = format!("{base}- [ ] Ask about SSO seats\n");
        let r = crate::api::call(&app, "topics.save_note", &json!({ "id": id, "text": mine, "base": base })).unwrap();
        assert_eq!(r["state"], "saving");
        let ob = r["outbox_id"].as_i64().unwrap();
        let http = FakeHttp::default();
        let note = json!({ "path": "Topics/Datadog renewal.md", "content": base, "stat": { "ctime": 1, "mtime": 2, "size": 3 },
                           "tags": [], "frontmatter": {}, "links": [], "backlinks": [], "unresolvedLinks": [] });
        http.on("GET", "/vault/Topics/Datadog%20renewal.md", 200, note.to_string());
        http.on("PUT", "/vault/Topics/Datadog%20renewal.md", 204, "");
        crate::outbox::process_one(&app, &http).unwrap();
        let s = crate::api::call(&app, "topics.note_status", &json!({ "outbox_id": ob })).unwrap();
        assert_eq!(s["state"], "saved");
        let put = http.requests().into_iter().find(|r| r.0 == "PUT").unwrap();
        assert_eq!(put.2.as_deref(), Some(mine.as_str()));
        // Topics without a note can't be edited.
        let q4 = topic_id(&app, "Q4 headcount");
        app.db().execute("UPDATE topics SET obsidian_path = NULL WHERE id = ?1", [q4]).unwrap();
        assert!(crate::api::call(&app, "topics.save_note", &json!({ "id": q4, "text": "x", "base": null })).is_err());
    }

    #[test]
    fn only_the_next_future_instance_of_a_series_is_on_the_timeline() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let conn = app.db();
        let src: i64 = conn.query_row("SELECT id FROM sources LIMIT 1", [], |r| r.get(0)).unwrap();
        for (ext, at) in [("s0", "2000-01-01T09:00:00"), ("s1", "2099-01-01T09:00:00"), ("s2", "2099-01-08T09:00:00")] {
            conn.execute(
                "INSERT INTO items (source_id, external_id, kind, title, occurred_at, seen, raw_json)
                 VALUES (?1, ?2, 'event', 'Weekly sync', ?3, 1, '{\"recurringEventId\":\"S\"}')",
                rusqlite::params![src, ext, at],
            )
            .unwrap();
            conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')", rusqlite::params![conn.last_insert_rowid(), id]).unwrap();
        }
        drop(conn);
        let v = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap();
        let n = v["chronology"].as_array().unwrap().iter().filter(|e| e["title"] == "Weekly sync").count();
        assert_eq!(n, 2);
    }

    #[test]
    fn wrong_links_leave_the_topic() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let before = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap()["chronology"].as_array().unwrap().len();
        app.db()
            .execute(
                "UPDATE topic_links SET state = 'wrong' WHERE topic_id = ?1 AND item_id = (SELECT min(item_id) FROM topic_links WHERE topic_id = ?1)",
                [id],
            )
            .unwrap();
        let after = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap()["chronology"].as_array().unwrap().len();
        assert_eq!(after, before - 1);
    }

    #[test]
    fn daystrip_life_blocks_are_untitled() {
        let (app, _dir) = app();
        let v = crate::api::call(&app, "daystrip.get", &json!({})).unwrap();
        let blocks = v["blocks"].as_array().unwrap();
        let life: Vec<_> = blocks.iter().filter(|b| b["life"] == true).collect();
        assert_eq!(life.len(), 3);
        for b in life {
            assert!(b["title"].is_null());
            assert!(b["attendees"].is_null());
            assert_eq!(b["buffer_min"], 15);
        }
    }

    #[test]
    fn daystrip_note_names_next_linked_meeting() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Alerting · P2");
        let v = crate::api::call(&app, "daystrip.get", &json!({ "topic_id": id })).unwrap();
        assert!(v["note"].as_str().unwrap().starts_with("No linked meetings today · next"));
        let dd = topic_id(&app, "Datadog renewal");
        let v = crate::api::call(&app, "daystrip.get", &json!({ "topic_id": dd })).unwrap();
        assert!(v["note"].is_null());
    }

    #[test]
    fn buffer_override_only_applies_to_life_blocks() {
        let (app, _dir) = app();
        let life: i64 = app.db().query_row("SELECT id FROM events WHERE is_life_busy = 1 LIMIT 1", [], |r| r.get(0)).unwrap();
        crate::api::call(&app, "daystrip.set_buffer", &json!({ "event_id": life, "minutes": 25 })).unwrap();
        let v = crate::api::call(&app, "daystrip.get", &json!({})).unwrap();
        let b = v["blocks"].as_array().unwrap().iter().find(|b| b["id"] == life).unwrap().clone();
        assert_eq!(b["buffer_min"], 25);
        assert!(crate::api::call(&app, "daystrip.set_buffer", &json!({ "event_id": life, "minutes": 999 })).is_err());
    }

    #[test]
    fn search_finds_items_and_topics_and_ignores_fts_syntax() {
        let (app, _dir) = app();
        let v = crate::api::call(&app, "search.query", &json!({ "q": "redlines" })).unwrap();
        assert!(!v["items"].as_array().unwrap().is_empty());
        let v = crate::api::call(&app, "search.query", &json!({ "q": "datadog" })).unwrap();
        assert_eq!(v["topics"][0]["title"], "Datadog renewal");
        // Raw FTS operators must not error.
        assert!(crate::api::call(&app, "search.query", &json!({ "q": "\"NEAR( OR *" })).is_ok());
    }
}

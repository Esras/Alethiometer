//! Topics (7a): the root object. Upcoming, Open, Chronology, People, and the
//! topic's Obsidian note.

use super::{arg_i64, dur_label, parse_ts, when_label, Pill};
use crate::api::unknown;
use crate::error::Result;
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct TopicRow {
    id: i64,
    name: String,
    category: String,
    new_count: i64,
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
        "list" => list(app),
        "get" => get(app, arg_i64(args, "id")?),
        _ => Err(unknown(name)),
    }
}

fn list(app: &App) -> Result<Value> {
    let conn = app.db();
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.category,
                (SELECT count(*) FROM topic_links tl JOIN items i ON i.id = tl.item_id
                  WHERE tl.topic_id = t.id AND tl.state = 'suggested' AND i.seen = 0)
           FROM topics t WHERE t.archived_at IS NULL ORDER BY t.name",
    )?;
    let rows = stmt
        .query_map([], |r| Ok(TopicRow { id: r.get(0)?, name: r.get(1)?, category: r.get(2)?, new_count: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!(rows))
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
                i.msg_count, i.owner
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
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

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
                    label: format!("due {}", when_label(d, now, false)),
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
        let has_time = is_event;
        let range = if r.kind == "thread" {
            r.ended.as_deref().and_then(parse_ts).map(|e| format!("{}–{}", at.format("%H:%M"), e.format("%H:%M")))
        } else {
            None
        };
        chron.push((t_str.clone(), Entry {
            item_id: r.id,
            when: when_label(t, now, has_time),
            today: t.date() == now.date(),
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

    let note = path.as_ref().map(|p| {
        let abs = app.vault.join(p);
        let text = std::fs::read_to_string(&abs).unwrap_or_default();
        json!({ "path": p, "abs_path": abs.to_string_lossy(), "text": text })
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
    fn datadog_topic_has_sections_and_note() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let v = crate::api::call(&app, "topics.get", &json!({ "id": id })).unwrap();
        assert_eq!(v["open"].as_array().unwrap().len(), 3);
        assert_eq!(v["people"].as_array().unwrap().len(), 3);
        assert!(v["chronology"].as_array().unwrap().len() >= 14);
        assert!(v["note"]["text"].as_str().unwrap().contains("## Where we are"));
        // Timeline is newest first.
        let whens: Vec<i64> = v["chronology"].as_array().unwrap().iter().map(|e| e["item_id"].as_i64().unwrap()).collect();
        assert!(!whens.is_empty());
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

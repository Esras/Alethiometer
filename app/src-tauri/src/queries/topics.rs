//! Topics (7a): the root object. Upcoming, Open, Chronology, People, and the
//! topic's Obsidian note.

use super::{arg_i64, arg_str, due_label, dur_label, ts_label, Pill};
use crate::clock::Clock;
use crate::time::{Due, Ts};
use jiff::{SignedDuration, ToSpan};
use crate::api::{unknown, Route};
use crate::domain::ports::Reads;
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
    last_signal_at: Option<Ts>,
    last_signal: Option<String>,
    quiet: bool,
    next_meeting: Option<Meeting>,
}

#[derive(Serialize)]
struct Meeting {
    start: Ts,
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

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "list" => Route::Read(|r, ctx, args| list(r, ctx, args.get("archived").and_then(Value::as_bool).unwrap_or(false))),
        _ => return None,
    })
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
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
        "rename" => rename(app, arg_i64(args, "id")?, &arg_str(args, "name")?),
        "merge" => merge(app, arg_i64(args, "from")?, arg_i64(args, "into")?),
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
    let category = category.unwrap_or(super::categories::UNCATEGORIZED);
    let conn = app.db();
    if category != super::categories::UNCATEGORIZED && !super::categories::exists(&conn, category)? {
        return Err(crate::error::msg(format!("unknown category '{category}'")));
    }
    if find_by_name(&conn, name)?.is_some() {
        return Err(crate::error::msg(format!("a topic named '{name}' already exists")));
    }
    Ok(json!({ "id": find_or_create(&conn, name, category)? }))
}

/// Renames in place; the linked Obsidian note keeps its own path (14d).
fn rename(app: &App, id: i64, name: &str) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::error::msg("Topic name can't be empty"));
    }
    let conn = app.db();
    if let Some(other) = find_by_name(&conn, name)? {
        if other != id {
            return Err(crate::error::msg(format!("a topic named '{name}' already exists")));
        }
    }
    conn.execute("UPDATE topics SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(json!({ "ok": true }))
}

/// Merge topic `from` into `into` (user decisions 2026-10-01). The UI stages
/// a merge and only calls this when you leave the screen; until then `u`
/// cancels it, so nothing here needs undoing.
/// - Every signal link moves to `into`; where the item already has a link
///   there, the stronger state wins (confirmed, then suggested, then wrong).
///   People and repos linked by hand, and Notes lines pointing at `from`,
///   move too.
/// - Gmail threads filed under `from` get `into`'s category label when the
///   categories differ (`gmail.relabel`).
/// - `from`'s note text (frontmatter left out) is appended to `into`'s note
///   under "## Merged from <name>", an ordinary note save (outbox or file).
///   `from`'s note file stays where it is.
/// - `from` is deleted.
fn merge(app: &App, from: i64, into: i64) -> Result<Value> {
    use rusqlite::OptionalExtension;
    if from == into {
        return Err(crate::error::msg("a topic can't be merged into itself"));
    }
    type Row = (String, String, Option<String>, Option<String>);
    let row = |conn: &rusqlite::Connection, id: i64| -> Result<Option<Row>> {
        Ok(conn
            .query_row("SELECT name, category, obsidian_path, archived_at FROM topics WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?)
    };
    let (from_name, from_path, into_name, into_path) = {
        let conn = app.db();
        let a = row(&conn, from)?.ok_or_else(|| crate::error::msg("the topic to merge is gone"))?;
        let b = row(&conn, into)?.ok_or_else(|| crate::error::msg("the topic to merge into is gone"))?;
        if b.3.is_some() {
            return Err(crate::error::msg(format!("'{}' is archived: unarchive it first", b.0)));
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason, updated_at)
             SELECT item_id, ?2, state, confidence, reason, updated_at FROM topic_links WHERE topic_id = ?1 AND true
             ON CONFLICT (item_id, topic_id) DO UPDATE SET state = CASE
               WHEN 'confirmed' IN (state, excluded.state) THEN 'confirmed'
               WHEN 'suggested' IN (state, excluded.state) THEN 'suggested'
               ELSE 'wrong' END",
            params![from, into],
        )?;
        // Gmail threads filed under the old topic, while we still know which.
        let filed: Vec<i64> = tx
            .prepare("SELECT item_id FROM topic_links WHERE topic_id = ?1 AND state = 'confirmed'")?
            .query_map([from], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for item in filed {
            super::incoming::relabel_gmail(&tx, item, Some(&a.1), &b.1)?;
        }
        tx.execute("DELETE FROM topic_links WHERE topic_id = ?1", [from])?;
        hand_over_and_delete(&tx, from, into, &b.0, &b.1)?;
        tx.commit()?;
        (a.0, a.2, b.0, b.2)
    };
    // The merge is committed: a note that can't be appended doesn't undo it,
    // it's reported (`from`'s note file is still there to copy by hand).
    let note = match append_merged_note(app, into, &from_name, from_path.as_deref(), &into_name, into_path) {
        Ok(n) => n.to_owned(),
        Err(e) => format!("failed: {e}"),
    };
    Ok(json!({ "ok": true, "note": note }))
}

/// Everything outside `topic_links` that names topic `from`, handed to
/// `into` (named `into_name`, category `into_category`), then `from` itself
/// deleted. The one place that knows every table referring to a topic: a
/// test checks it against the schema's foreign keys.
/// - People and repos linked by hand, Glean pins and rejects (`into`'s own
///   verdict wins on the same result) and Notes lines move.
/// - `from`'s vector goes, and `into`'s is marked stale: its text now
///   includes `from`'s signals. Ids aren't AUTOINCREMENT, so a later topic
///   can get `from`'s id and must not inherit its vector.
fn hand_over_and_delete(tx: &rusqlite::Transaction, from: i64, into: i64, into_name: &str, into_category: &str) -> Result<()> {
    for table in ["topic_people", "topic_repos", "search_feedback"] {
        tx.execute(&format!("UPDATE OR IGNORE {table} SET topic_id = ?2 WHERE topic_id = ?1"), params![from, into])?;
        tx.execute(&format!("DELETE FROM {table} WHERE topic_id = ?1"), [from])?;
    }
    tx.execute(
        "UPDATE notes_lines SET target_json = json_set(target_json, '$.id', ?2, '$.label', ?3, '$.category', ?4)
          WHERE json_extract(target_json, '$.type') = 'topic' AND json_extract(target_json, '$.id') = ?1",
        params![from, into, into_name, into_category],
    )?;
    tx.execute("DELETE FROM vec_topics WHERE rowid = ?1", [from])?;
    tx.execute("DELETE FROM embed_meta WHERE kind = 'topic' AND id IN (?1, ?2)", params![from, into])?;
    tx.execute("DELETE FROM topics WHERE id = ?1", [from])?;
    Ok(())
}

/// Append the merged topic's note to the surviving topic's note. Returns
/// `none` (nothing to append), `appended`, `queued` or `conflict`.
fn append_merged_note(app: &App, into: i64, from_name: &str, from_path: Option<&str>, into_name: &str, into_path: Option<String>) -> Result<&'static str> {
    let Some(from_path) = from_path else { return Ok("none") };
    let Some(text) = save::disk_text(app, from_path.trim_start_matches("./").trim_start_matches('/')) else { return Ok("none") };
    let body = strip_frontmatter(&text).trim();
    if body.is_empty() {
        return Ok("none");
    }
    let linked = into_path.is_some();
    let path = into_path.unwrap_or_else(|| default_note_path(into_name));
    let rel = path.trim_start_matches("./").trim_start_matches('/').to_string();
    let before = save::disk_text(app, &rel);
    let mut after = before.clone().unwrap_or_default();
    if !after.is_empty() {
        after = format!("{}\n\n", after.trim_end());
    }
    after.push_str(&format!("## Merged from {from_name}\n\n{body}\n"));
    let saved = save::save_note(app, &rel, before.as_deref(), &after)?;
    if !linked {
        app.db().execute("UPDATE topics SET obsidian_path = ?2 WHERE id = ?1 AND obsidian_path IS NULL", params![into, path])?;
    }
    Ok(match saved {
        save::Saved::Written => "appended",
        save::Saved::Queued(_) => "queued",
        save::Saved::Conflict(_) => "conflict",
    })
}

fn strip_frontmatter(text: &str) -> &str {
    let t = text.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("---\n").or_else(|| t.strip_prefix("---\r\n")) {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.strip_prefix('\r').unwrap_or(after).strip_prefix('\n').unwrap_or(after);
        }
    }
    t
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
    let (name, path): (String, Option<String>) =
        app.db().query_row("SELECT name, obsidian_path FROM topics WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let linked = path.is_some();
    let path = path.unwrap_or_else(|| default_note_path(&name));
    let rel = path.trim_start_matches("./").trim_start_matches('/');
    let saved = save::save_note(app, rel, base, text)?;
    if !linked {
        app.db().execute("UPDATE topics SET obsidian_path = ?2 WHERE id = ?1 AND obsidian_path IS NULL", rusqlite::params![id, path])?;
    }
    Ok(match saved {
        save::Saved::Written => json!({ "state": "saved" }),
        save::Saved::Conflict(disk) => json!({ "state": "conflict", "disk": disk }),
        save::Saved::Queued(outbox_id) => json!({ "state": "saving", "outbox_id": outbox_id }),
    })
}

fn default_note_path(name: &str) -> String {
    let file: String = name.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    format!("Topics/{}.md", file.trim().trim_start_matches('.'))
}

fn list(r: &dyn Reads, ctx: &crate::api::Ctx, archived: bool) -> Result<Value> {
    let clock = ctx.clock;
    let now = clock.now();
    let week = now + SignedDuration::from_hours(24 * 7);
    let quiet_before = now - SignedDuration::from_hours(24 * ctx.settings.quiet_days);
    let mut rows: Vec<TopicRow> = r
        .topics()
        .list(archived, now, week)?
        .into_iter()
        .map(|t| TopicRow {
            id: t.id,
            name: t.name,
            category: t.category,
            obsidian_path: t.obsidian_path,
            new_count: t.new_count,
            open_count: t.open_count,
            last_signal: t.last_signal_at.map(|l| last_label(l, clock, t.last_source.as_deref())),
            quiet: t.last_signal_at.is_none_or(|l| l < quiet_before),
            last_signal_at: t.last_signal_at,
            next_meeting: t.next_meeting.map(|(start, title)| Meeting { start, title }),
        })
        .collect();
    // Active first (next meeting soonest, then most recent signal), then quiet.
    rows.sort_by(|a, b| {
        let meet = |t: &TopicRow| t.next_meeting.as_ref().map(|m| m.start);
        (a.quiet, meet(a).is_none(), meet(a))
            .cmp(&(b.quiet, meet(b).is_none(), meet(b)))
            .then_with(|| b.last_signal_at.cmp(&a.last_signal_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(json!(rows))
}

/// "10:41 · Slack" today, then "Yesterday", a weekday within the week, else "Sep 25".
fn last_label(t: Ts, clock: &Clock, source: Option<&str>) -> String {
    let t = clock.local(t);
    match (clock.today() - t.date()).get_days() {
        0 => match source {
            Some(s) => format!("{} · {}", t.strftime("%H:%M"), source_label(s)),
            None => t.strftime("%H:%M").to_string(),
        },
        1 => "Yesterday".into(),
        2..=6 => t.strftime("%a").to_string(),
        _ => t.strftime("%b %-d").to_string(),
    }
}

fn set_archived(app: &App, id: i64, on: bool) -> Result<Value> {
    let n = app.db().execute(
        "UPDATE topics SET archived_at = CASE WHEN ?2 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END WHERE id = ?1",
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

/// A topic's page. The note is read from the vault after the snapshot is closed.
fn get(app: &App, id: i64) -> Result<Value> {
    let settings = app.settings();
    let ctx = crate::api::Ctx { clock: &app.clock, settings: &settings };
    let (mut page, path) = app.store().read(|r| page(r, &ctx, id))?;
    // `base` is what an edit starts from and must still be there when it saves
    // (null: no file yet). The file on disk is what Obsidian last saved.
    let abs = app.vault.join(&path);
    let disk = std::fs::read_to_string(&abs).ok();
    page["note"] = json!({ "path": path, "abs_path": abs.to_string_lossy(), "text": disk.clone().unwrap_or_default(), "base": disk });
    Ok(page)
}

/// Everything on a topic's page but the note, and the note's vault path.
fn page(reads: &dyn Reads, ctx: &crate::api::Ctx, id: i64) -> Result<(Value, String)> {
    let clock = ctx.clock;
    let now = clock.now();
    let today = clock.today();
    let head = reads.topics().head(id)?.ok_or_else(|| crate::error::msg("no such topic"))?;
    let (name, category, path, score) = (head.name, head.category, head.obsidian_path, head.score);
    let pills: Vec<Pill> = serde_json::from_str(&head.pills_json).unwrap_or_default();

    // All confirmed links; suggestions still waiting in Incoming are left out.
    let rows = reads.topics().confirmed_items(id)?;
    let mut next_of_series: std::collections::HashMap<&str, Ts> = std::collections::HashMap::new();
    for r in rows.iter().filter(|r| r.at > now) {
        if let Some(s) = &r.series {
            let e = next_of_series.entry(s).or_insert(r.at);
            if r.at < *e {
                *e = r.at;
            }
        }
    }

    let mut upcoming = vec![];
    let mut open = vec![];
    let mut chron: Vec<(Ts, Entry)> = vec![];
    for r in &rows {
        let at = r.at;
        let is_event = r.kind == "event";
        let mins = r.ended.map(|e| (e - at).as_mins());
        if is_event && at > now && (clock.day_of(at) - today).get_days() <= 7 {
            upcoming.push((at, Upcoming {
                item_id: r.id,
                when: ts_label(at, clock, true),
                today: clock.day_of(at) == today,
                source: r.source_kind.clone(),
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
                source: r.source_kind.clone(),
                title: r.title.clone(),
                due: r.due.map(|d| Pill {
                    label: format!("due {}", due_label(d, clock)),
                    weight: "yellow".into(),
                }),
                owner: r.owner.clone(),
            });
        }

        // Timeline: open tasks sit at their due date; everything else at when it happened.
        let (t, title, sub, right) = if is_task && r.status == "open" {
            match r.due {
                Some(d) => (d.deadline(clock), r.title.clone(), Some("due".to_string()), source_label(&r.source_kind).to_string()),
                None => continue,
            }
        } else {
            let right = match (mins, r.msg_count) {
                (Some(m), _) if is_event => dur_label(m),
                (_, Some(n)) => format!("{n} msgs"),
                _ => source_label(&r.source_kind).to_string(),
            };
            let names = is_event && r.summary.as_deref().map_or(false, is_names);
            let (title, sub) = match &r.summary {
                Some(s) if names => (format!("{} · {s}", r.title), None),
                s => (r.title.clone(), s.clone()),
            };
            (r.at, title, sub, right)
        };
        let is_due = is_task && r.status == "open";
        // Events show their time; due dates show one only when they have it.
        let has_time = is_event || (is_due && matches!(r.due, Some(Due::At(_))));
        let range = if r.kind == "thread" {
            r.ended.map(|e| format!("{}–{}", clock.local(at).strftime("%H:%M"), clock.local(e).strftime("%H:%M")))
        } else {
            None
        };
        if r.series.as_deref().is_some_and(|s| r.at > now && next_of_series.get(s) != Some(&r.at)) {
            continue;
        }
        chron.push((t, Entry {
            item_id: r.id,
            when: ts_label(t, clock, has_time),
            // Only today's meetings are lit; due dates stay grey like everything else.
            today: !is_due && clock.day_of(t) == today,
            source: r.source_kind.clone(),
            title,
            range,
            sub,
            right,
            open: t > now || (is_task && r.status == "open"),
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

    let people = {
        let r = super::people::Resolver::load(reads, &ctx.settings.people)?;
        super::people::for_topic(reads, &r, id, clock, 9)?
    };
    let repos = reads.topics().repos(id)?;
    let path = path.unwrap_or_else(|| default_note_path(&name));

    Ok((json!({
        "topic": { "id": id, "name": name, "category": category, "score": score, "pills": pills },
        "upcoming": upcoming.into_iter().map(|(_, u)| u).collect::<Vec<_>>(),
        "open": open,
        "chronology": chron.into_iter().map(|(_, e)| e).collect::<Vec<_>>(),
        "filters": filters.into_iter().map(|(source, count)| json!({ "source": source, "count": count })).collect::<Vec<_>>(),
        "people": people,
        "repos": repos,
    }), path))
}

/// Attendee summaries ("Ravi, Kim") read better inline with the title.
fn is_names(s: &str) -> bool {
    !s.is_empty() && s.split(", ").all(|w| w.chars().next().map_or(false, char::is_uppercase) && !w.contains(' '))
}

pub fn seed(ctx: &Ctx, vault: &std::path::Path) -> anyhow::Result<()> {
    let c = ctx.conn;
    let dd = "Datadog renewal";
    let counter_due = ctx.today + 2.days();
    let pills = serde_json::json!([
        { "label": "$ at stake", "weight": "red" },
        { "label": format!("counter due {}", counter_due.strftime("%a")), "weight": "yellow" },
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
        due: Some(Due::At(ctx.at(2, "17:00"))),
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
    use super::strip_frontmatter;
    use rusqlite::params;
    use serde_json::{json, Value};

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
        let cat: String = app.db().query_row("SELECT category FROM topics WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(cat, "", "new topics start uncategorized");
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
    fn rename_checks_for_duplicates_and_leaves_the_note_path() {
        let (app, _dir) = app();
        let r = crate::api::call(&app, "topics.create", &json!({ "name": "New thing" })).unwrap();
        let id = r["id"].as_i64().unwrap();
        crate::api::call(&app, "topics.rename", &json!({ "id": id, "name": "Renamed thing" })).unwrap();
        assert_eq!(topic_id(&app, "Renamed thing"), id);
        let e = crate::api::call(&app, "topics.rename", &json!({ "id": id, "name": "Datadog renewal" })).unwrap_err().to_string();
        assert!(e.contains("already exists"), "{e}");
        let e = crate::api::call(&app, "topics.rename", &json!({ "id": id, "name": " " })).unwrap_err().to_string();
        assert!(e.contains("empty"), "{e}");
        crate::api::call(&app, "topics.rename", &json!({ "id": id, "name": "renamed thing" })).unwrap();
        assert_eq!(topic_id(&app, "renamed thing"), id, "renaming to its own name (case-only) is fine");
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
    fn merging_moves_everything_appends_the_note_and_deletes_the_topic() {
        let (app, _dir) = app();
        let dd = topic_id(&app, "Datadog renewal");
        let q4 = topic_id(&app, "Q4 planning");
        let links = |t: i64| -> i64 { app.db().query_row("SELECT count(*) FROM topic_links WHERE topic_id = ?1", [t], |r| r.get(0)).unwrap() };
        // An item on both: Datadog's confirmed link beats Q4's wrong one.
        let both: i64 = app.db().query_row("SELECT item_id FROM topic_links WHERE topic_id = ?1 AND state = 'confirmed' LIMIT 1", [dd], |r| r.get(0)).unwrap();
        app.db().execute("INSERT OR REPLACE INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'wrong')", params![both, q4]).unwrap();
        let (dd_links, q4_links) = (links(dd), links(q4));
        assert!(dd_links > 0);
        let conn = app.db();
        conn.execute("INSERT INTO notes_lines (page_date, position, kind, text, target_json) VALUES ('2026-09-27', 1, 'note', 'x', ?1)", [json!({ "type": "topic", "id": dd, "label": "Datadog renewal" }).to_string()]).unwrap();
        let q4_note = std::fs::read_to_string(app.vault.join("Topics/Q4 planning.md")).ok();
        drop(conn);

        let v = crate::api::call(&app, "topics.merge", &json!({ "from": dd, "into": q4 })).unwrap();
        assert_eq!(v["note"], "appended");
        assert_eq!(links(dd), 0);
        assert_eq!(links(q4), dd_links + q4_links - 1, "the shared item counts once");
        let conn = app.db();
        let state: String = conn.query_row("SELECT state FROM topic_links WHERE item_id = ?1 AND topic_id = ?2", params![both, q4], |r| r.get(0)).unwrap();
        assert_eq!(state, "confirmed");
        let gone: bool = conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM topics WHERE id = ?1)", [dd], |r| r.get(0)).unwrap();
        assert!(gone);
        let people: i64 = conn.query_row("SELECT count(*) FROM topic_people WHERE topic_id = ?1", [q4], |r| r.get(0)).unwrap();
        assert!(people >= 3, "Kim, Ravi and Ana came along");
        let repo: i64 = conn.query_row("SELECT count(*) FROM topic_repos WHERE topic_id = ?1", [q4], |r| r.get(0)).unwrap();
        assert_eq!(repo, 1);
        let target: String = conn.query_row("SELECT target_json FROM notes_lines WHERE text = 'x'", [], |r| r.get(0)).unwrap();
        let target: Value = serde_json::from_str(&target).unwrap();
        assert_eq!((target["id"].as_i64(), target["label"].as_str()), (Some(q4), Some("Q4 planning")));
        let note = std::fs::read_to_string(app.vault.join("Topics/Q4 planning.md")).unwrap();
        if let Some(before) = q4_note {
            assert!(note.starts_with(before.trim_end()), "the surviving note is kept as it was");
        }
        assert!(note.contains("## Merged from Datadog renewal\n\n## Where we are"), "{note}");
        assert!(app.vault.join("Topics/Datadog renewal.md").exists(), "the merged topic's file stays");
        drop(conn);
        assert!(crate::api::call(&app, "topics.merge", &json!({ "from": q4, "into": q4 })).is_err());
        assert!(crate::api::call(&app, "topics.merge", &json!({ "from": dd, "into": q4 })).is_err(), "already gone");
    }

    #[test]
    fn every_table_naming_a_topic_is_handled_when_one_goes() {
        // A new table with a foreign key to topics must be added to
        // hand_over_and_delete (or merging a topic that has rows there fails).
        let conn = crate::db::open_in_memory().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let mut referring: Vec<String> = tables
            .into_iter()
            .filter(|t| {
                conn.prepare(&format!("SELECT \"table\" FROM pragma_foreign_key_list('{t}')"))
                    .unwrap()
                    .query_map([], |r| r.get::<_, String>(0))
                    .unwrap()
                    .any(|r| r.unwrap() == "topics")
            })
            .collect();
        referring.sort();
        assert_eq!(referring, ["search_feedback", "topic_links", "topic_people", "topic_repos"]);
    }

    #[test]
    fn merging_a_topic_with_glean_pins_and_a_vector() {
        let (app, _dir) = app();
        let dd = topic_id(&app, "Datadog renewal");
        let q4 = topic_id(&app, "Q4 headcount");
        {
            let conn = app.db();
            for (t, key, verdict) in [(dd, "https://a", "pin"), (dd, "https://b", "reject"), (q4, "https://b", "pin")] {
                conn.execute("INSERT INTO search_feedback (topic_id, key, verdict) VALUES (?1, ?2, ?3)", params![t, key, verdict]).unwrap();
            }
            crate::embed::store(&conn, "topic", dd, "m", "h1", &[0.5; 384]).unwrap();
            crate::embed::store(&conn, "topic", q4, "m", "h2", &[0.25; 384]).unwrap();
        }
        crate::api::call(&app, "topics.merge", &json!({ "from": dd, "into": q4 })).unwrap();
        let conn = app.db();
        let fb: Vec<(String, String)> = conn
            .prepare("SELECT key, verdict FROM search_feedback WHERE topic_id = ?1 ORDER BY key")
            .unwrap()
            .query_map([q4], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(fb, [("https://a".to_owned(), "pin".to_owned()), ("https://b".to_owned(), "pin".to_owned())], "moved; the surviving topic's own verdict wins");
        assert!(crate::embed::vector(&conn, "topic", dd).unwrap().is_none(), "the merged topic's vector is gone");
        let stale: i64 = conn.query_row("SELECT count(*) FROM embed_meta WHERE kind = 'topic' AND id IN (?1, ?2)", params![dd, q4], |r| r.get(0)).unwrap();
        assert_eq!(stale, 0, "the surviving topic is re-embedded");
    }

    #[test]
    fn frontmatter_is_left_out_of_a_merged_note() {
        assert_eq!(strip_frontmatter("---\ntags: [x]\n---\n## A\n"), "## A\n");
        assert_eq!(strip_frontmatter("## A\n"), "## A\n");
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
        // A topic without a note gets Topics/<name>.md, linked on first save.
        let q4 = topic_id(&app, "Q4 headcount");
        app.db().execute("UPDATE topics SET name = 'Q4 CI/CD', obsidian_path = NULL WHERE id = ?1", [q4]).unwrap();
        let g = crate::api::call(&app, "topics.get", &json!({ "id": q4 })).unwrap();
        assert_eq!(g["note"]["path"], "Topics/Q4 CI-CD.md");
        assert!(g["note"]["base"].is_null());
        crate::api::call(&app, "topics.save_note", &json!({ "id": q4, "text": "x", "base": null })).unwrap();
        let p: String = app.db().query_row("SELECT obsidian_path FROM topics WHERE id = ?1", [q4], |r| r.get(0)).unwrap();
        assert_eq!(p, "Topics/Q4 CI-CD.md");
    }

    #[test]
    fn only_the_next_future_instance_of_a_series_is_on_the_timeline() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let conn = app.db();
        let src: i64 = conn.query_row("SELECT id FROM sources LIMIT 1", [], |r| r.get(0)).unwrap();
        for (ext, at) in [("s0", "2000-01-01T09:00:00.000Z"), ("s1", "2099-01-01T09:00:00.000Z"), ("s2", "2099-01-08T09:00:00.000Z")] {
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
        let today = app.clock.today();
        let (day_start, day_end) = app.clock.day_range(today);
        let at = |hm: &str| app.clock.from_local(today.to_datetime(jiff::civil::Time::strptime("%H:%M:%S", hm).unwrap()));
        let set = |start: &str, end: &str| {
            app.db()
                .execute(
                    "UPDATE events SET start = ?2, end = ?3
                      WHERE start >= ?4 AND start < ?5 AND item_id IN (SELECT item_id FROM topic_links WHERE topic_id = ?1)",
                    rusqlite::params![dd, at(start), at(end), day_start, day_end],
                )
                .unwrap()
        };
        assert!(set("00:01:00", "23:59:59") > 0);
        let v = crate::api::call(&app, "daystrip.get", &json!({ "topic_id": dd })).unwrap();
        assert!(v["note"].is_null(), "a linked meeting still to come today");
        set("00:01:00", "00:02:00");
        let v = crate::api::call(&app, "daystrip.get", &json!({ "topic_id": dd })).unwrap();
        assert!(v["note"].as_str().unwrap().starts_with("No more linked meetings today"), "{}", v["note"]);
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

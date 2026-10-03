//! Builds the text the work AI sees, locally, from work sources only.
//!
//! Boundary rule 4: nothing from a `personal` source goes into a prompt. Every
//! query joins `sources` and keeps `tag = 'work'`, and each row is checked again
//! in Rust so a future query edit can't quietly widen it. Life blocks are left
//! out entirely (not even as "busy"); they aren't needed to answer questions.
//!
//! Context is capped at `BUDGET` characters, filled newest first, so a busy
//! topic can't blow up the request.

use crate::error::Result;
use rusqlite::{params, Connection};
use std::fmt::Write;
use std::path::Path;

/// Rough cap on context size (~6k tokens), plenty for a concise answer.
pub const BUDGET: usize = 24_000;
/// Share of the budget the topic note may take.
const NOTE_CAP: usize = 6_000;
/// Share of the budget the topic's people may take.
const PEOPLE_CAP: usize = 2_500;
const BODY_CAP: usize = 600;
const MSG_CAP: usize = 300;
const MSGS_PER_ITEM: usize = 6;

#[derive(Debug, Default)]
pub struct Context {
    pub text: String,
    /// Item ids whose content is in `text`, so the UI can cite them.
    pub used: Vec<i64>,
}

impl Context {
    /// Append a block if it fits. Returns false once the budget is spent.
    fn push(&mut self, block: &str, item: Option<i64>) -> bool {
        if self.text.len() + block.len() > BUDGET {
            return false;
        }
        self.text.push_str(block);
        if let Some(id) = item {
            if !self.used.contains(&id) {
                self.used.push(id);
            }
        }
        true
    }
}

/// First `n` characters, on a char boundary, with an ellipsis if cut.
pub fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// A topic's Obsidian note, or "" if it has none or it can't be read.
pub fn topic_note(conn: &Connection, vault: &Path, topic_id: i64) -> Result<(String, String)> {
    let (name, path): (String, Option<String>) =
        conn.query_row("SELECT name, obsidian_path FROM topics WHERE id = ?1", [topic_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let text = path.map(|p| std::fs::read_to_string(vault.join(p)).unwrap_or_default()).unwrap_or_default();
    Ok((name, text))
}

struct ItemRow {
    id: i64,
    tag: String,
    source: String,
    kind: String,
    title: String,
    summary: Option<String>,
    body: String,
    at: String,
    ended: Option<String>,
    due: Option<String>,
    status: String,
    owner: Option<String>,
}

const ITEM_COLS: &str = "i.id, s.tag, s.kind, i.kind, i.title, i.summary, i.body, i.occurred_at, i.ended_at, i.due_at, i.status, i.owner";

fn item_row(r: &rusqlite::Row) -> rusqlite::Result<ItemRow> {
    Ok(ItemRow {
        id: r.get(0)?,
        tag: r.get(1)?,
        source: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        summary: r.get(5)?,
        body: r.get(6)?,
        at: r.get(7)?,
        ended: r.get(8)?,
        due: r.get(9)?,
        status: r.get(10)?,
        owner: r.get(11)?,
    })
}

fn is_open_task(r: &ItemRow) -> bool {
    matches!(r.kind.as_str(), "task" | "issue" | "email") && r.owner.is_some() && r.status == "open"
}

fn item_block(conn: &Connection, r: &ItemRow) -> Result<String> {
    let mut b = format!("\n[item {}] {} {} · {}", r.id, r.source, r.kind, r.at.replace('T', " "));
    if let Some(e) = &r.ended {
        let _ = write!(b, " to {}", e.replace('T', " "));
    }
    let _ = write!(b, " · {}", if is_open_task(r) { "OPEN TASK" } else { &r.status });
    if let Some(d) = &r.due {
        let _ = write!(b, " · due {}", d.replace('T', " "));
    }
    if let Some(o) = &r.owner {
        let _ = write!(b, " · owner {o}");
    }
    let _ = write!(b, "\nTitle: {}\n", clip(&r.title, 300));
    if let Some(s) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = writeln!(b, "Summary: {}", clip(s, 300));
    }
    if !r.body.trim().is_empty() {
        let _ = writeln!(b, "Body: {}", clip(&r.body, BODY_CAP));
    }
    // Most recent messages, shown oldest to newest.
    let mut stmt = conn.prepare("SELECT at, author, text FROM item_messages WHERE item_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2")?;
    let mut msgs = stmt
        .query_map(params![r.id, MSGS_PER_ITEM as i64], |m| Ok((m.get::<_, String>(0)?, m.get::<_, String>(1)?, m.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    msgs.reverse();
    for (at, author, text) in msgs {
        let _ = writeln!(b, "  {} {author}: {}", at.get(11..16).unwrap_or(&at), clip(&text, MSG_CAP));
    }
    Ok(b)
}

fn topic_rows(conn: &Connection, topic_id: i64, state: &str) -> Result<Vec<ItemRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ITEM_COLS}
           FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
          WHERE tl.topic_id = ?1 AND tl.state = ?2 AND s.tag = 'work' AND (?2 = 'confirmed' OR i.seen = 0)
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let rows = stmt.query_map(params![topic_id, state], item_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().filter(|r| r.tag == "work").collect())
}

/// Context for questions about one topic: its note, its new signals still in
/// Incoming (suggested, not confirmed), open tasks, then confirmed items newest first.
pub fn for_topic(conn: &Connection, vault: &Path, topic_id: i64) -> Result<Context> {
    let (name, note) = topic_note(conn, vault, topic_id)?;
    let mut ctx = Context::default();
    ctx.push(&format!("Topic: {name}\n"), None);
    if !note.trim().is_empty() {
        ctx.push(&format!("\nTopic note (the user's Obsidian note):\n{}\n", clip(&note, NOTE_CAP)), None);
    }
    let now = chrono::Local::now().naive_local();
    let people = crate::queries::people::agent_context(conn, vault, topic_id, now)?;
    if !people.is_empty() {
        ctx.push(&format!("\nPeople on this topic (from their People notes):\n{}", clip(&people, PEOPLE_CAP)), None);
    }
    let suggested = topic_rows(conn, topic_id, "suggested")?;
    if !suggested.is_empty() {
        ctx.push("\nNew signals suggested for this topic, not yet confirmed by the user (newest first):\n", None);
        for r in &suggested {
            if !ctx.push(&item_block(conn, r)?, Some(r.id)) {
                break;
            }
        }
    }
    let rows = topic_rows(conn, topic_id, "confirmed")?;
    // Open tasks first so the budget never drops them; then everything else newest first.
    let (open, rest): (Vec<&ItemRow>, Vec<&ItemRow>) = rows.iter().partition(|r| is_open_task(r));
    ctx.push("\nItems linked to this topic (newest first):\n", None);
    for r in open.into_iter().chain(rest) {
        if !ctx.push(&item_block(conn, r)?, Some(r.id)) {
            break;
        }
    }
    Ok(ctx)
}

/// Context for reviewing the topic list (Topics index): every active topic with its
/// note gist, size, last activity and a few recent work item titles.
pub fn for_topics(conn: &Connection, vault: &Path) -> Result<Context> {
    let mut ctx = Context::default();
    ctx.push("Active topics (id, name, linked work items, last activity, note gist, recent items):\n", None);
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.obsidian_path,
                (SELECT count(*) FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = t.id AND tl.state = 'confirmed' AND s.tag = 'work'),
                (SELECT max(i.occurred_at) FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = t.id AND tl.state = 'confirmed' AND s.tag = 'work')
           FROM topics t WHERE t.archived_at IS NULL ORDER BY t.name",
    )?;
    let topics = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut recent = conn.prepare(
        "SELECT i.title, s.tag FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
          WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'
          ORDER BY i.occurred_at DESC LIMIT 5",
    )?;
    for (id, name, path, n, last) in topics {
        let titles: Vec<String> = recent
            .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .filter(|(_, tag)| tag == "work")
            .map(|(t, _)| clip(&t, 80))
            .collect();
        let gist = path.map(|p| note_gist(&std::fs::read_to_string(vault.join(p)).unwrap_or_default())).unwrap_or_default();
        let last = last.as_deref().and_then(|l| l.get(..10)).unwrap_or("never");
        let block = format!("- [topic {id}] {name} · {n} items · last {last}\n  note: {gist}\n  recent: {}\n", titles.join("; "));
        if !ctx.push(&block, None) {
            break;
        }
    }
    Ok(ctx)
}

/// Context for "today": today's work calendar and work items touched today.
pub fn for_today(conn: &Connection, today: chrono::NaiveDate) -> Result<Context> {
    let day = today.format("%Y-%m-%d").to_string();
    let mut ctx = Context::default();
    ctx.push(&format!("Today is {}.\n\nWork calendar today:\n", today.format("%A %Y-%m-%d")), None);

    let mut stmt = conn.prepare(
        "SELECT e.item_id, s.tag, e.start, e.end, e.title, e.attendees, e.tentative
           FROM events e JOIN sources s ON s.id = e.source_id
          WHERE s.tag = 'work' AND e.is_life_busy = 0 AND e.title IS NOT NULL AND e.start >= ?1 AND e.start < date(?1, '+1 day')
          ORDER BY e.start",
    )?;
    let events = stmt
        .query_map([&day], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (item, tag, start, end, title, attendees, tentative) in events {
        if tag != "work" {
            continue;
        }
        let mut line = format!("- {}–{} {}", start.get(11..16).unwrap_or(&start), end.get(11..16).unwrap_or(&end), clip(&title, 200));
        if let Some(a) = attendees.filter(|a| !a.is_empty()) {
            let _ = write!(line, " (with {})", clip(&a, 200));
        }
        if tentative {
            line.push_str(" [tentative]");
        }
        if let Some(id) = item {
            let _ = write!(line, " [item {id}]");
        }
        line.push('\n');
        if !ctx.push(&line, item) {
            return Ok(ctx);
        }
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT {ITEM_COLS}
           FROM items i JOIN sources s ON s.id = i.source_id
          WHERE s.tag = 'work' AND i.kind != 'event'
            AND (substr(i.occurred_at, 1, 10) = ?1 OR substr(i.ended_at, 1, 10) = ?1
                 OR EXISTS (SELECT 1 FROM item_messages m WHERE m.item_id = i.id AND substr(m.at, 1, 10) = ?1))
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let rows: Vec<ItemRow> =
        stmt.query_map([&day], item_row)?.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().filter(|r| r.tag == "work").collect();
    ctx.push("\nWork items touched today (newest first):\n", None);
    for r in &rows {
        if !ctx.push(&item_block(conn, r)?, Some(r.id)) {
            break;
        }
    }
    Ok(ctx)
}

/// One-line description of a topic from the first lines of its note
/// (frontmatter and blank lines skipped), for topic suggestions.
pub fn note_gist(note: &str) -> String {
    let mut lines = note.lines().map(str::trim).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for l in lines.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    let parts: Vec<&str> = lines
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_start_matches(['#', '-', '*', ' ']).trim_start_matches("[ ]").trim_start_matches("[x]").trim())
        .filter(|l| !l.is_empty())
        .take(4)
        .collect();
    clip(&parts.join(" / "), 200)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::state::App;

    /// Test app with AI *unconfigured* (no models), so each test opts in.
    pub fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let mut settings = Settings::default();
        settings.ai.small.model.clear();
        settings.ai.large.model.clear();
        let app = App::from_parts(conn, settings, dir.path()).unwrap();
        (app, dir)
    }

    pub fn topic_id(app: &App, name: &str) -> i64 {
        app.db().query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    }

    /// Plant personal content directly in the DB, bypassing apply()'s guard,
    /// linked to a topic and dated today, to prove the context builder drops it.
    pub fn plant_personal(app: &App, topic: i64) -> i64 {
        let conn = app.db();
        let src: i64 = conn.query_row("SELECT id FROM sources WHERE tag = 'personal' LIMIT 1", [], |r| r.get(0)).unwrap();
        let today = app.clock.now().date().format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, body, summary, occurred_at, status, owner, seen)
             VALUES (?1, 'p-1', 'task', 'PERSONAL-SECRET dentist', 'PERSONAL-SECRET body', 'PERSONAL-SECRET summary', ?2, 'open', 'you', 0)",
            params![src, format!("{today}T09:30:00")],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO item_messages (item_id, at, author, text) VALUES (?1, ?2, 'me', 'PERSONAL-SECRET msg')", params![id, format!("{today}T09:31:00")]).unwrap();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')", params![id, topic]).unwrap();
        // A titled event on the personal source too (apply() would never allow this).
        conn.execute(
            "INSERT INTO events (item_id, source_id, start, end, title, category, is_life_busy) VALUES (?1, ?2, ?3, ?4, 'PERSONAL-SECRET appt', 'life', 0)",
            params![id, src, format!("{today}T15:00:00"), format!("{today}T16:00:00")],
        )
        .unwrap();
        id
    }

    #[test]
    fn topic_context_includes_its_new_suggested_signals() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        {
            let conn = app.db();
            let src: i64 = conn.query_row("SELECT id FROM sources WHERE tag = 'work' LIMIT 1", [], |r| r.get(0)).unwrap();
            for (ext, title, seen) in [("n1", "FRESH-SUGGESTION renewal quote", 0), ("n2", "OLD-SUGGESTION already triaged", 1)] {
                conn.execute(
                    "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, seen) VALUES (?1, ?2, 'message', ?3, '2026-09-01T09:00:00', 'open', ?4)",
                    params![src, ext, title, seen],
                )
                .unwrap();
                conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", params![conn.last_insert_rowid(), id]).unwrap();
            }
        }
        let ctx = for_topic(&app.db(), &app.vault, id).unwrap();
        assert!(ctx.text.contains("not yet confirmed"));
        assert!(ctx.text.contains("FRESH-SUGGESTION"));
        assert!(!ctx.text.contains("OLD-SUGGESTION"));
    }

    #[test]
    fn topic_context_has_note_items_and_messages() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let ctx = for_topic(&app.db(), &app.vault, id).unwrap();
        assert!(ctx.text.contains("Topic: Datadog renewal"));
        assert!(ctx.text.contains("True-down at 12 months"), "note text included");
        assert!(ctx.text.contains("[item "));
        assert!(!ctx.used.is_empty());
        assert!(ctx.text.len() <= BUDGET);
        assert!(ctx.text.contains("People on this topic"));
        assert!(ctx.text.contains("- Kim (Procurement), note People/Kim.md"), "{}", ctx.text);
    }

    #[test]
    fn contexts_exclude_personal_sources() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let personal = plant_personal(&app, id);
        let today = app.clock.now().date();
        let topic = for_topic(&app.db(), &app.vault, id).unwrap();
        let day = for_today(&app.db(), today).unwrap();
        for ctx in [topic, day] {
            assert!(!ctx.text.contains("PERSONAL-SECRET"), "{}", ctx.text);
            assert!(!ctx.used.contains(&personal));
        }
    }

    #[test]
    fn today_has_work_meetings_but_no_life_blocks() {
        let (app, _dir) = app();
        let ctx = for_today(&app.db(), app.clock.now().date()).unwrap();
        assert!(ctx.text.contains("Q4 headcount"), "{}", ctx.text);
        assert!(ctx.text.contains("Acme call") && ctx.text.contains("[tentative]"));
        // Life blocks (12:00–13:00 etc.) have no title and must not appear at all.
        assert!(!ctx.text.contains("12:00–13:00"), "{}", ctx.text);
        assert!(!ctx.text.to_lowercase().contains("busy"));
    }

    #[test]
    fn budget_caps_context_newest_first() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Q4 headcount");
        {
            let conn = app.db();
            let src: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0)).unwrap();
            for n in 0..200 {
                conn.execute(
                    "INSERT INTO items (source_id, external_id, kind, title, body, occurred_at, status) VALUES (?1, ?2, 'thread', ?3, ?4, ?5, 'open')",
                    params![src, format!("bulk-{n}"), format!("Bulk {n}"), "x".repeat(500), format!("2020-01-01T{:02}:{:02}:00", n / 60, n % 60)],
                )
                .unwrap();
                let item = conn.last_insert_rowid();
                conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')", params![item, id]).unwrap();
            }
        }
        let ctx = for_topic(&app.db(), &app.vault, id).unwrap();
        assert!(ctx.text.len() <= BUDGET);
        assert!(ctx.text.contains("Bulk 199"), "newest kept");
        assert!(!ctx.text.contains("Bulk 0\n"), "oldest dropped");
    }

    #[test]
    fn gist_skips_frontmatter_and_markup() {
        let g = note_gist("---\ntags: [x]\n---\n\n## Where we are\n- Offer: 12% off\n- [ ] True-down\n");
        assert_eq!(g, "Where we are / Offer: 12% off / True-down");
    }
}

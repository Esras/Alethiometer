//! Builds the text the work AI sees, locally, from work sources only.
//!
//! Boundary rule 4: nothing from a `personal` source goes into a prompt. Every
//! query joins `sources` and keeps `tag = 'work'`, and each row is checked again
//! in Rust so a future query edit can't quietly widen it. Life blocks are left
//! out entirely (not even as "busy"); they aren't needed to answer questions.
//!
//! Context is capped at `BUDGET` characters, filled newest first, so a busy
//! topic can't blow up the request.

use crate::clock::Clock;
use crate::domain::context::ContextItem;
use crate::domain::ports::Reads;
use crate::error::{msg, Result};
use crate::state::App;
use crate::time::{Date, Due, Ts};
use std::collections::HashMap;
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

/// Vault notes a prompt quotes, read after the snapshot that named them has
/// closed (the database is never held across a file read).
#[derive(Debug, Default)]
pub struct Notes(HashMap<String, String>);

impl Notes {
    pub fn read(vault: &Path, paths: impl IntoIterator<Item = String>) -> Notes {
        Notes(paths.into_iter().map(|p| (p.clone(), std::fs::read_to_string(vault.join(&p)).unwrap_or_default())).collect())
    }

    /// The note's text, or "" if it wasn't read or isn't there.
    pub fn get(&self, path: &str) -> &str {
        self.0.get(path).map_or("", String::as_str)
    }
}

/// A topic's name and its Obsidian note ("" if it has none or it can't be read).
pub fn topic_note(app: &App, topic_id: i64) -> Result<(String, String)> {
    let head = app.store().read(|r| r.topics().head(topic_id))?.ok_or_else(|| msg("no such topic"))?;
    let text = head.obsidian_path.map(|p| std::fs::read_to_string(app.vault.join(p)).unwrap_or_default()).unwrap_or_default();
    Ok((head.name, text))
}

/// "2026-10-03 10:52" here.
fn local(clock: &Clock, t: Ts) -> String {
    clock.local(t).strftime("%Y-%m-%d %H:%M").to_string()
}

fn is_open_task(r: &ContextItem) -> bool {
    matches!(r.kind.as_str(), "task" | "issue" | "email") && r.owner.is_some() && r.status == "open"
}

fn item_block(reads: &dyn Reads, clock: &Clock, r: &ContextItem) -> Result<String> {
    let mut b = format!("\n[item {}] {} {} · {}", r.id, r.source_kind, r.kind, local(clock, r.at));
    if let Some(e) = r.ended {
        let _ = write!(b, " to {}", local(clock, e));
    }
    let _ = write!(b, " · {}", if is_open_task(r) { "OPEN TASK" } else { &r.status });
    match r.due {
        Some(Due::At(t)) => {
            let _ = write!(b, " · due {}", local(clock, t));
        }
        Some(Due::Day(d)) => {
            let _ = write!(b, " · due {d}");
        }
        None => {}
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
    for (at, author, text) in reads.context().latest_messages(r.id, MSGS_PER_ITEM)? {
        let _ = writeln!(b, "  {} {author}: {}", clock.local(at).strftime("%H:%M"), clip(&text, MSG_CAP));
    }
    Ok(b)
}

/// A topic's work items in `state`, checked again for the work tag.
fn topic_rows(reads: &dyn Reads, topic_id: i64, state: &str) -> Result<Vec<ContextItem>> {
    Ok(reads.context().topic_items(topic_id, state)?.into_iter().filter(|r| r.tag == "work").collect())
}

/// Context for questions about one topic: its note, its new signals still in
/// Incoming (suggested, not confirmed), open tasks, then confirmed items newest first.
pub fn for_topic(app: &App, topic_id: i64) -> Result<Context> {
    let paths = app.store().read(|r| {
        let mut paths: Vec<String> = r.topics().head(topic_id)?.and_then(|h| h.obsidian_path).into_iter().collect();
        paths.extend(crate::queries::people::agent_note_paths(r, topic_id, &app.clock)?);
        Ok(paths)
    })?;
    let notes = Notes::read(&app.vault, paths);
    app.store().read(|r| topic_context(r, &app.clock, &notes, topic_id))
}

fn topic_context(reads: &dyn Reads, clock: &Clock, notes: &Notes, topic_id: i64) -> Result<Context> {
    let head = reads.topics().head(topic_id)?.ok_or_else(|| msg("no such topic"))?;
    let note = head.obsidian_path.as_deref().map_or("", |p| notes.get(p));
    let mut ctx = Context::default();
    ctx.push(&format!("Topic: {}\n", head.name), None);
    if !note.trim().is_empty() {
        ctx.push(&format!("\nTopic note (the user's Obsidian note):\n{}\n", clip(note, NOTE_CAP)), None);
    }
    let people = crate::queries::people::agent_context(reads, notes, topic_id, clock)?;
    if !people.is_empty() {
        ctx.push(&format!("\nPeople on this topic (from their People notes):\n{}", clip(&people, PEOPLE_CAP)), None);
    }
    let suggested = topic_rows(reads, topic_id, "suggested")?;
    if !suggested.is_empty() {
        ctx.push("\nNew signals suggested for this topic, not yet confirmed by the user (newest first):\n", None);
        for r in &suggested {
            if !ctx.push(&item_block(reads, clock, r)?, Some(r.id)) {
                break;
            }
        }
    }
    let rows = topic_rows(reads, topic_id, "confirmed")?;
    // Open tasks first so the budget never drops them; then everything else newest first.
    let (open, rest): (Vec<&ContextItem>, Vec<&ContextItem>) = rows.iter().partition(|r| is_open_task(r));
    ctx.push("\nItems linked to this topic (newest first):\n", None);
    for r in open.into_iter().chain(rest) {
        if !ctx.push(&item_block(reads, clock, r)?, Some(r.id)) {
            break;
        }
    }
    Ok(ctx)
}

/// Context for reviewing the topic list (Topics index): every active topic with its
/// note gist, size, last activity and a few recent work item titles.
pub fn for_topics(app: &App) -> Result<Context> {
    let topics = app.store().read(|r| r.context().topics_overview())?;
    let notes = Notes::read(&app.vault, topics.iter().filter_map(|t| t.note_path.clone()));
    app.store().read(|r| {
        let mut ctx = Context::default();
        ctx.push("Active topics (id, name, linked work items, last activity, note gist, recent items):\n", None);
        for t in &topics {
            let titles: Vec<String> = r.context().recent_titles(t.id, 5)?.iter().map(|t| clip(t, 80)).collect();
            let gist = t.note_path.as_deref().map(|p| note_gist(notes.get(p))).unwrap_or_default();
            let last = t.last.map(|l| app.clock.day_of(l).to_string()).unwrap_or_else(|| "never".into());
            let block = format!("- [topic {}] {} · {} items · last {last}\n  note: {gist}\n  recent: {}\n", t.id, t.name, t.items, titles.join("; "));
            if !ctx.push(&block, None) {
                break;
            }
        }
        Ok(ctx)
    })
}

/// Context for "today": today's work calendar and work items touched today.
pub fn for_today(reads: &dyn Reads, clock: &Clock, today: Date) -> Result<Context> {
    let (day_start, day_end) = clock.day_range(today);
    let mut ctx = Context::default();
    ctx.push(&format!("Today is {}.\n\nWork calendar today:\n", today.strftime("%A %Y-%m-%d")), None);
    for e in reads.context().work_events(day_start, day_end)? {
        if e.tag != "work" {
            continue;
        }
        let mut line = format!("- {}–{} {}", clock.local(e.start).strftime("%H:%M"), clock.local(e.end).strftime("%H:%M"), clip(&e.title, 200));
        if let Some(a) = e.attendees.filter(|a| !a.is_empty()) {
            let _ = write!(line, " (with {})", clip(&a, 200));
        }
        if e.tentative {
            line.push_str(" [tentative]");
        }
        if let Some(id) = e.item_id {
            let _ = write!(line, " [item {id}]");
        }
        line.push('\n');
        if !ctx.push(&line, e.item_id) {
            return Ok(ctx);
        }
    }
    let rows: Vec<ContextItem> = reads.context().touched(day_start, day_end)?.into_iter().filter(|r| r.tag == "work").collect();
    ctx.push("\nWork items touched today (newest first):\n", None);
    for r in &rows {
        if !ctx.push(&item_block(reads, clock, r)?, Some(r.id)) {
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
    use rusqlite::params;
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
        let at = |h: i8, m: i8| app.clock.from_local(app.clock.today().at(h, m, 0, 0));
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, body, summary, occurred_at, status, owner, seen)
             VALUES (?1, 'p-1', 'task', 'PERSONAL-SECRET dentist', 'PERSONAL-SECRET body', 'PERSONAL-SECRET summary', ?2, 'open', 'you', 0)",
            params![src, at(9, 30)],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO item_messages (item_id, at, author, text) VALUES (?1, ?2, 'me', 'PERSONAL-SECRET msg')", params![id, at(9, 31)]).unwrap();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')", params![id, topic]).unwrap();
        // A titled event on the personal source too (apply() would never allow this).
        conn.execute(
            "INSERT INTO events (item_id, source_id, start, end, title, category, is_life_busy) VALUES (?1, ?2, ?3, ?4, 'PERSONAL-SECRET appt', 'life', 0)",
            params![id, src, at(15, 0), at(16, 0)],
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
                    "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, seen) VALUES (?1, ?2, 'message', ?3, '2026-09-01T09:00:00.000Z', 'open', ?4)",
                    params![src, ext, title, seen],
                )
                .unwrap();
                conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", params![conn.last_insert_rowid(), id]).unwrap();
            }
        }
        let ctx = for_topic(&app, id).unwrap();
        assert!(ctx.text.contains("not yet confirmed"));
        assert!(ctx.text.contains("FRESH-SUGGESTION"));
        assert!(!ctx.text.contains("OLD-SUGGESTION"));
    }

    #[test]
    fn topic_context_has_note_items_and_messages() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Datadog renewal");
        let ctx = for_topic(&app, id).unwrap();
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
        let today = app.clock.today();
        let topic = for_topic(&app, id).unwrap();
        let day = app.store().read(|r| for_today(r, &app.clock, today)).unwrap();
        for ctx in [topic, day] {
            assert!(!ctx.text.contains("PERSONAL-SECRET"), "{}", ctx.text);
            assert!(!ctx.used.contains(&personal));
        }
    }

    #[test]
    fn today_has_work_meetings_but_no_life_blocks() {
        let (app, _dir) = app();
        let ctx = app.store().read(|r| for_today(r, &app.clock, app.clock.today())).unwrap();
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
                    params![src, format!("bulk-{n}"), format!("Bulk {n}"), "x".repeat(500), format!("2020-01-01T{:02}:{:02}:00.000Z", n / 60, n % 60)],
                )
                .unwrap();
                let item = conn.last_insert_rowid();
                conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')", params![item, id]).unwrap();
            }
        }
        let ctx = for_topic(&app, id).unwrap();
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

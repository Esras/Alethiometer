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
pub use crate::queries::note_text::{clip, note_gist, Notes};
use crate::state::App;
use crate::time::{Date, Due, Ts};
use jiff::ToSpan;
use std::fmt::Write;

/// Rough cap on context size (~6k tokens), plenty for a concise answer.
pub const BUDGET: usize = 24_000;
/// A topic's items past the detailed ones go in as one line each, up to this much more: so a question about a
/// big topic ("split this up") sees all of it, not only the newest few dozen.
const INDEX_BUDGET: usize = 40_000;
/// Share of the budget the topic note may take.
const NOTE_CAP: usize = 6_000;
/// Share of the budget the topic's people may take.
const PEOPLE_CAP: usize = 2_500;
const BODY_CAP: usize = 600;
const MSG_CAP: usize = 300;
const MSGS_PER_ITEM: usize = 6;

/// How much of an item `item_block` shows: the prompt's own context, or one item the model asked for.
#[derive(Clone, Copy)]
enum Depth {
    Brief,
    Full,
}

#[derive(Debug, Default)]
pub struct Context {
    pub text: String,
    /// Item ids whose content is in `text`, so the UI can cite them.
    pub used: Vec<i64>,
}

impl Context {
    /// Append a block if it fits. Returns false once the budget is spent.
    fn push(&mut self, block: &str, item: Option<i64>) -> bool {
        self.push_within(BUDGET, block, item)
    }

    /// `push` against a cap of its own (the one-line index past the detailed items).
    fn push_within(&mut self, cap: usize, block: &str, item: Option<i64>) -> bool {
        if self.text.len() + block.len() > cap {
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

/// A topic's name and its Obsidian note ("" if it has none or it can't be read).
pub fn topic_note(app: &App, topic_id: i64) -> Result<(String, String)> {
    let head = app
        .store()
        .read(|r| r.topics().head(topic_id))?
        .ok_or_else(|| msg("no such topic"))?;
    let text = head
        .obsidian_path
        .map(|p| std::fs::read_to_string(app.vault.join(p)).unwrap_or_default())
        .unwrap_or_default();
    Ok((head.name, text))
}

/// "2026-10-03 10:52" here.
fn local(clock: &Clock, t: Ts) -> String {
    clock.local(t).strftime("%Y-%m-%d %H:%M").to_string()
}

fn is_open_task(r: &ContextItem) -> bool {
    matches!(r.kind.as_str(), "task" | "issue" | "email") && r.owner.is_some() && r.status == "open"
}

fn item_block(reads: &dyn Reads, clock: &Clock, r: &ContextItem, depth: Depth) -> Result<String> {
    let (body_cap, msgs, msg_cap) = match depth {
        Depth::Brief => (BODY_CAP, MSGS_PER_ITEM, MSG_CAP),
        Depth::Full => (4_000, 40, 1_000),
    };
    let mut b = format!(
        "\n[item {}] {} {} · {}",
        r.id,
        r.source_kind,
        r.kind,
        local(clock, r.at)
    );
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
        let _ = writeln!(b, "Body: {}", clip(&r.body, body_cap));
    }
    // Most recent messages, shown oldest to newest.
    for (at, author, text) in reads.context().latest_messages(r.id, msgs)? {
        let _ = writeln!(b, "  {} {author}: {}", local(clock, at), clip(&text, msg_cap));
    }
    Ok(b)
}

/// A topic's work items in `state`, checked again for the work tag.
fn topic_rows(reads: &dyn Reads, topic_id: i64, state: &str) -> Result<Vec<ContextItem>> {
    Ok(reads
        .context()
        .topic_items(topic_id, state)?
        .into_iter()
        .filter(|r| r.tag == "work")
        .collect())
}

/// Context for questions about one topic: its note, its new signals still in
/// Incoming (suggested, not confirmed), open tasks, then confirmed items newest first.
pub fn for_topic(app: &App, topic_id: i64) -> Result<Context> {
    let paths = app.store().read(|r| {
        let mut paths: Vec<String> = r
            .topics()
            .head(topic_id)?
            .and_then(|h| h.obsidian_path)
            .into_iter()
            .collect();
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
    let rows = topic_rows(reads, topic_id, "confirmed")?;
    let suggested = topic_rows(reads, topic_id, "suggested")?;
    ctx.push(&overview(&rows, suggested.len()), None);
    if !note.trim().is_empty() {
        ctx.push(
            &format!("\nTopic note (the user's Obsidian note):\n{}\n", clip(note, NOTE_CAP)),
            None,
        );
    }
    let people = crate::queries::people::agent_context(reads, notes, topic_id, clock)?;
    if !people.is_empty() {
        ctx.push(
            &format!(
                "\nPeople on this topic (from their People notes):\n{}",
                clip(&people, PEOPLE_CAP)
            ),
            None,
        );
    }
    if !suggested.is_empty() {
        ctx.push(
            "\nNew signals suggested for this topic, not yet confirmed by the user (newest first):\n",
            None,
        );
        for r in &suggested {
            if !ctx.push(&item_block(reads, clock, r, Depth::Brief)?, Some(r.id)) {
                break;
            }
        }
    }
    // Open tasks first so the budget never drops them; then everything else newest first.
    let (open, rest): (Vec<&ContextItem>, Vec<&ContextItem>) = rows.iter().partition(|r| is_open_task(r));
    ctx.push("\nItems linked to this topic (newest first):\n", None);
    let mut shown = 0;
    for r in open.iter().chain(&rest) {
        if !ctx.push(&item_block(reads, clock, r, Depth::Brief)?, Some(r.id)) {
            break;
        }
        shown += 1;
    }
    let left: Vec<&&ContextItem> = open.iter().chain(&rest).skip(shown).collect();
    if !left.is_empty() {
        let cap = BUDGET + INDEX_BUDGET;
        ctx.push_within(
            cap,
            &format!(
                "\nThe other {} linked items, one line each (newest first):\n",
                left.len()
            ),
            None,
        );
        let mut listed = 0;
        for r in &left {
            let line = format!(
                "[item {}] {} {} {} \u{b7} {}\n",
                r.id,
                clock.day_of(r.at),
                r.source_kind,
                r.kind,
                clip(&r.title, 100)
            );
            if !ctx.push_within(cap, &line, Some(r.id)) {
                break;
            }
            listed += 1;
        }
        if listed < left.len() {
            ctx.push_within(
                cap + 200,
                &format!("({} older items not listed.)\n", left.len() - listed),
                None,
            );
        }
    }
    Ok(ctx)
}

/// A topic in numbers: its confirmed items by source and by month, and how many suggestions wait.
fn overview(rows: &[ContextItem], suggested: usize) -> String {
    use std::collections::BTreeMap;
    let mut by_source: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_month: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        *by_source.entry(r.source_kind.as_str()).or_default() += 1;
        *by_month.entry(r.at.to_db().chars().take(7).collect()).or_default() += 1;
    }
    let mut sources: Vec<_> = by_source.into_iter().collect();
    sources.sort_by(|a, b| b.1.cmp(&a.1));
    let list = |v: Vec<String>| if v.is_empty() { "none".to_owned() } else { v.join(", ") };
    format!(
        "Linked items: {} confirmed, {suggested} suggested and not yet confirmed.\nBy source: {}\nBy month (UTC): {}\n",
        rows.len(),
        list(sources.iter().map(|(s, n)| format!("{s} {n}")).collect()),
        list(by_month.iter().rev().map(|(m, n)| format!("{m} {n}")).collect()),
    )
}

/// Context for reviewing the topic list (Topics index): every active topic with its
/// note gist, size, last activity and a few recent work item titles.
pub fn for_topics(app: &App) -> Result<Context> {
    let topics = app.store().read(|r| r.context().topics_overview())?;
    let notes = Notes::read(&app.vault, topics.iter().filter_map(|t| t.note_path.clone()));
    app.store().read(|r| {
        let mut ctx = Context::default();
        ctx.push(
            "Active topics (id, name, linked work items, suggestions the user rejected, last activity, note gist, recent items):\n",
            None,
        );
        for t in &topics {
            let titles: Vec<String> = r
                .context()
                .recent_titles(t.id, 5)?
                .iter()
                .map(|t| clip(t, 80))
                .collect();
            let gist = t
                .note_path
                .as_deref()
                .map(|p| note_gist(notes.get(p)))
                .unwrap_or_default();
            let last = t
                .last
                .map(|l| app.clock.day_of(l).to_string())
                .unwrap_or_else(|| "never".into());
            let block = format!(
                "- [topic {}] {} · {} items · {} rejected · last {last}\n  note: {gist}\n  recent: {}\n",
                t.id,
                t.name,
                t.items,
                t.wrong,
                titles.join("; ")
            );
            if !ctx.push(&block, None) {
                break;
            }
        }
        Ok(ctx)
    })
}

/// Context for "today": today's work calendar and work items touched today.
pub fn for_today(reads: &dyn Reads, clock: &Clock, today: Date) -> Result<Context> {
    for_days(reads, clock, today, today, today + 1.days())
}

/// The days `from` to `to` (exclusive): calendar, items in detail, then one line per other item, shared out by day.
pub fn for_days(reads: &dyn Reads, clock: &Clock, today: Date, from: Date, to: Date) -> Result<Context> {
    let (day_start, day_end) = (clock.day_range(from).0, clock.day_range(to).0);
    let one_day = to == from + 1.days();
    let when = if one_day && from == today {
        "today".to_string()
    } else if one_day {
        format!("on {}", from.strftime("%A %Y-%m-%d"))
    } else {
        format!(
            "from {} to {}",
            from.strftime("%a %Y-%m-%d"),
            (to - 1.days()).strftime("%a %Y-%m-%d")
        )
    };
    let days: Vec<Date> = std::iter::successors(Some(from), |d| Some(*d + 1.days()))
        .take_while(|d| *d < to)
        .collect();
    let day_of = |t: Ts| {
        let d = clock.local(t).date().clamp(from, to - 1.days());
        days.iter().position(|x| *x == d).unwrap_or(0)
    };
    let mut ctx = Context::default();
    ctx.push(
        &format!("Today is {}.\n\nWork calendar {when}:\n", today.strftime("%A %Y-%m-%d")),
        None,
    );
    let mut events: Vec<Vec<Line>> = vec![vec![]; days.len()];
    for e in reads.context().work_events(day_start, day_end)? {
        if e.tag != "work" {
            continue;
        }
        let mut line = format!(
            "- {}–{} {}",
            clock.local(e.start).strftime("%H:%M"),
            clock.local(e.end).strftime("%H:%M"),
            clip(&e.title, 200)
        );
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
        events[day_of(e.start)].push((line, e.item_id));
    }
    by_day(&mut ctx, BUDGET / 3, &days, &events, "meetings");

    let rows: Vec<ContextItem> = reads
        .context()
        .touched(day_start, day_end)?
        .into_iter()
        .filter(|r| r.tag == "work")
        .collect();
    let mut detail: Vec<Vec<Line>> = vec![vec![]; days.len()];
    for r in rows.iter().filter(|r| !r.dismissed) {
        detail[day_of(r.ended.unwrap_or(r.at))].push((item_block(reads, clock, r, Depth::Brief)?, Some(r.id)));
    }
    ctx.push(
        &format!("\nWork items touched {when}, in detail (newest first within a day):\n"),
        None,
    );
    let left = BUDGET.saturating_sub(ctx.text.len());
    let shown = by_day(&mut ctx, left, &days, &detail, "");

    let mut index: Vec<Vec<Line>> = vec![vec![]; days.len()];
    for r in rows.iter().filter(|r| !shown.contains(&r.id)) {
        let line = format!(
            "- [item {}] {} {}: {} · {}{}\n",
            r.id,
            r.source_kind,
            r.kind,
            clip(&r.title, 120),
            if is_open_task(r) { "OPEN TASK" } else { &r.status },
            if r.dismissed { " [dismissed]" } else { "" }
        );
        index[day_of(r.ended.unwrap_or(r.at))].push((line, Some(r.id)));
    }
    if index.iter().any(|d| !d.is_empty()) {
        ctx.push_within(
            usize::MAX,
            &format!(
                "\nOther work items touched {when}, one line each (newest first within a day; get_item reads one in full). \
                 [dismissed]: dismissed by the user without filing under a topic (low importance; mention only if asked, \
                 and never as a count):\n"
            ),
            None,
        );
        by_day(&mut ctx, INDEX_BUDGET, &days, &index, "items");
    }
    Ok(ctx)
}

/// One work item in full for the ask's `get_item` tool; None if it isn't a work item.
pub fn item_detail(reads: &dyn Reads, clock: &Clock, id: i64) -> Result<Option<String>> {
    match reads.context().work_item(id)? {
        Some(r) => Ok(Some(item_block(reads, clock, &r, Depth::Full)?)),
        None => Ok(None),
    }
}

/// A block for the prompt and the item it shows, if any.
type Line = (String, Option<i64>);

/// Each day's lines under its date, an even share of `cap` per day (unused share rolls forward); returns items pushed.
fn by_day(ctx: &mut Context, cap: usize, days: &[Date], lines: &[Vec<Line>], more: &str) -> Vec<i64> {
    let start = ctx.text.len();
    let mut pushed = vec![];
    for (i, (day, ls)) in days.iter().zip(lines).enumerate() {
        if ls.is_empty() {
            continue;
        }
        let share = cap.saturating_sub(ctx.text.len() - start) / (days.len() - i);
        let limit = ctx.text.len() + share;
        if days.len() > 1 {
            ctx.push_within(usize::MAX, &format!("{}:\n", day.strftime("%a %Y-%m-%d")), None);
        }
        let mut cut = 0;
        for (l, id) in ls {
            if ctx.push_within(limit, l, *id) {
                pushed.extend(*id);
            } else {
                cut += 1;
            }
        }
        if cut > 0 && !more.is_empty() {
            let s = if days.len() > 1 { " this day" } else { "" };
            ctx.push_within(
                usize::MAX,
                &format!("- (+{cut} more {more}{s}; get_day reads them)\n"),
                None,
            );
        }
    }
    pushed
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::state::App;
    use rusqlite::params;

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
        app.db()
            .query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0))
            .unwrap()
    }

    /// Plant personal content directly in the DB, bypassing apply()'s guard,
    /// linked to a topic and dated today, to prove the context builder drops it.
    pub fn plant_personal(app: &App, topic: i64) -> i64 {
        let conn = app.db();
        let src: i64 = conn
            .query_row("SELECT id FROM sources WHERE tag = 'personal' LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let at = |h: i8, m: i8| app.clock.from_local(app.clock.today().at(h, m, 0, 0));
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, body, summary, occurred_at, status, owner, seen)
             VALUES (?1, 'p-1', 'task', 'PERSONAL-SECRET dentist', 'PERSONAL-SECRET body', 'PERSONAL-SECRET summary', ?2, 'open', 'you', 0)",
            params![src, at(9, 30)],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO item_messages (item_id, at, author, text) VALUES (?1, ?2, 'me', 'PERSONAL-SECRET msg')",
            params![id, at(9, 31)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')",
            params![id, topic],
        )
        .unwrap();
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
            let src: i64 = conn
                .query_row("SELECT id FROM sources WHERE tag = 'work' LIMIT 1", [], |r| r.get(0))
                .unwrap();
            for (ext, title, seen) in [
                ("n1", "FRESH-SUGGESTION renewal quote", 0),
                ("n2", "OLD-SUGGESTION already triaged", 1),
            ] {
                conn.execute(
                    "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, seen) VALUES (?1, ?2, 'message', ?3, '2026-09-01T09:00:00.000Z', 'open', ?4)",
                    params![src, ext, title, seen],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
                    params![conn.last_insert_rowid(), id],
                )
                .unwrap();
            }
        }
        let ctx = for_topic(&app, id).unwrap();
        assert!(ctx.text.contains("not yet confirmed"));
        assert!(ctx.text.contains("FRESH-SUGGESTION"));
        assert!(!ctx.text.contains("OLD-SUGGESTION"));
    }

    #[test]
    fn today_leaves_out_agent_approvals_and_demotes_dismissed_items() {
        let (app, _dir) = app();
        let conn = app.db();
        let at = app.clock.from_local(app.clock.today().at(10, 0, 0, 0));
        conn.execute(
            "INSERT INTO sources (kind, tag, name) VALUES ('agents', 'work', 'Agents')",
            [],
        )
        .unwrap();
        let agents = conn.last_insert_rowid();
        let work: i64 = conn
            .query_row(
                "SELECT id FROM sources WHERE tag = 'work' AND kind != 'agents' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        for (src, ext, kind, title, seen) in [
            (agents, "a1", "approval", "APPROVAL-NOISE run ls", 1),
            (work, "d1", "message", "DISMISSED-ONE newsletter", 1),
            (work, "k1", "message", "KEPT-ONE status", 0),
        ] {
            conn.execute(
                "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, seen) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6)",
                params![src, ext, kind, title, at, seen],
            )
            .unwrap();
        }
        drop(conn);
        let ctx = app
            .store()
            .read(|r| for_today(r, &app.clock, app.clock.today()))
            .unwrap();
        assert!(!ctx.text.contains("APPROVAL-NOISE"));
        let (kept, dismissed) = (
            ctx.text.find("KEPT-ONE").unwrap(),
            ctx.text.find("DISMISSED-ONE").unwrap(),
        );
        assert!(
            kept < dismissed && ctx.text.contains("DISMISSED-ONE newsletter \u{b7} open [dismissed]"),
            "{}",
            ctx.text
        );
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
        assert!(
            ctx.text.contains("- Kim (Procurement), note People/Kim.md"),
            "{}",
            ctx.text
        );
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
        let ctx = app
            .store()
            .read(|r| for_today(r, &app.clock, app.clock.today()))
            .unwrap();
        assert!(ctx.text.contains("Q4 headcount"), "{}", ctx.text);
        assert!(ctx.text.contains("Acme call") && ctx.text.contains("[tentative]"));
        // Life blocks (12:00–13:00 etc.) have no title and must not appear at all.
        assert!(!ctx.text.contains("12:00–13:00"), "{}", ctx.text);
        assert!(!ctx.text.to_lowercase().contains("busy"));
    }

    #[test]
    fn a_period_has_its_days_meetings_dated_and_no_others() {
        let (app, _dir) = app();
        let today = app.clock.today();
        let week = app
            .store()
            .read(|r| for_days(r, &app.clock, today, today - 2.days(), today + 5.days()))
            .unwrap();
        assert!(week.text.contains("Work calendar from "), "{}", week.text);
        assert!(week.text.contains(&today.strftime("%a %Y-%m-%d:\n").to_string()) && week.text.contains("Acme call"));
        let later = app
            .store()
            .read(|r| for_days(r, &app.clock, today, today + 30.days(), today + 37.days()))
            .unwrap();
        assert!(!later.text.contains("Acme call"), "{}", later.text);
    }

    #[test]
    fn by_day_gives_every_day_a_share_and_counts_the_rest() {
        let days: Vec<Date> = (0..3).map(|i| jiff::civil::date(2026, 11, 9) + i.days()).collect();
        let line = |d: i64, n: i64| (format!("{:<99}\n", format!("day{d} line{n}")), Some(d * 100 + n));
        let lines: Vec<Vec<Line>> = (0..3).map(|d| (0..10).map(|n| line(d, n)).collect()).collect();
        let mut ctx = Context::default();
        let pushed = by_day(&mut ctx, 960, &days, &lines, "items");
        for d in 0..3 {
            assert!(pushed.iter().filter(|id| **id / 100 == d).count() >= 2, "{}", ctx.text);
        }
        assert!(
            ctx.text.contains("Mon 2026-11-09:\n") && ctx.text.contains("more items this day"),
            "{}",
            ctx.text
        );
    }

    #[test]
    fn budget_caps_context_newest_first() {
        let (app, _dir) = app();
        let id = topic_id(&app, "Q4 headcount");
        {
            let conn = app.db();
            let src: i64 = conn
                .query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0))
                .unwrap();
            for n in 0..200 {
                conn.execute(
                    "INSERT INTO items (source_id, external_id, kind, title, body, occurred_at, status) VALUES (?1, ?2, 'thread', ?3, ?4, ?5, 'open')",
                    params![src, format!("bulk-{n}"), format!("Bulk {n}"), "x".repeat(500), format!("2020-01-01T{:02}:{:02}:00.000Z", n / 60, n % 60)],
                )
                .unwrap();
                let item = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'confirmed')",
                    params![item, id],
                )
                .unwrap();
            }
        }
        let ctx = for_topic(&app, id).unwrap();
        assert!(ctx.text.len() <= BUDGET + INDEX_BUDGET + 200);
        assert!(ctx.text.contains("Bulk 199"), "newest kept");
        let (detail, index) = ctx
            .text
            .split_once("linked items, one line each")
            .expect("the rest as an index");
        assert!(detail.len() <= BUDGET, "the detailed part keeps its budget");
        assert!(!detail.contains("Bulk 0\n"), "the oldest aren't in detail");
        assert!(index.contains("Bulk 0\n"), "but every item is listed: {index}");
        assert!(
            ctx.text.contains("200 confirmed") || ctx.text.contains("Linked items: 2"),
            "the topic in numbers"
        );
        assert!(ctx.text.contains("2020-01 200"), "by month");
    }
}

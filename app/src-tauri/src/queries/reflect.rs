//! Reflect (9b day, 9c week, 9d nudge). Built for stopping: the reflection note
//! takes the page, beside it a short factual brief and what to carry forward.
//!
//! Each scope (day/week/month/quarter) and lens (both/work/life) has its own
//! Obsidian note in the work vault. Alethiometer owns these files but still never
//! clobbers them: a save only lands if the file on disk still matches what the
//! UI loaded; otherwise it reports a conflict and writes nothing.
//!
//! Each level rolls up the one below through `reflections.summary`, a one-line
//! summary derived from the first line of the note (never written by the AI).
//!
//! Life lens = busy-hour totals only: untitled Life blocks, no titles, no
//! carry-forward, no AI.
//!
//! What's real vs sample in the day brief:
//! - plan: the day's calendar events (real).
//! - did: the same events, using actual join/leave times from the event item's
//!   `raw_json` (`actual_start`/`actual_end`) where known, clipped to now. The
//!   sample data sets these on two meetings; there is no activity tracking.
//! - decisions: items of kind `decision` that day (sample: two Obsidian lines).
//! - jotted: Notes lines for that day.

use crate::domain::sample::{ItemPatch, SampleData};
use super::{arg_i64, arg_str, dur_label, minute_of_day, parse_date, parse_ts};
use crate::api::{unknown, Route};
use crate::error::{msg, Result};
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use crate::sync::connectors::obsidian::save;
use crate::clock::Clock;
use crate::time::{Date, Ts};
use jiff::civil::DateTime;
use jiff::ToSpan;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::settings::Settings;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub fn route(name: &str) -> Option<Route> {
    match name {
        "get" => Some(Route::ReadFiles(get)),
        _ => None,
    }
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "save" => save(app, args),
        "save_status" => save::status(app, arg_i64(args, "outbox_id")?),
        "keep_mine" => save::keep_mine(app, arg_i64(args, "outbox_id")?),
        "carry" => {
            let today = app.clock.today();
            app.store().write(|uow| carry(uow, today, arg_i64(args, "item_id")?, &arg_str(args, "period")?, &arg_str(args, "action")?))
        }
        "nudge" => nudge(app),
        "nudge_answer" => nudge_answer(app, &arg_str(args, "answer")?),
        _ => Err(unknown(name)),
    }
}

// ---------------------------------------------------------------- periods

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Day,
    Week,
    Month,
    Quarter,
}

impl Scope {
    fn parse(s: &str) -> Result<Scope> {
        Ok(match s {
            "day" => Scope::Day,
            "week" => Scope::Week,
            "month" => Scope::Month,
            "quarter" => Scope::Quarter,
            _ => return Err(msg(format!("bad scope '{s}'"))),
        })
    }
    fn key(self) -> &'static str {
        match self {
            Scope::Day => "day",
            Scope::Week => "week",
            Scope::Month => "month",
            Scope::Quarter => "quarter",
        }
    }
    fn folder(self) -> &'static str {
        match self {
            Scope::Day => "Daily",
            Scope::Week => "Weekly",
            Scope::Month => "Monthly",
            Scope::Quarter => "Quarterly",
        }
    }
    fn child(self) -> Option<Scope> {
        match self {
            Scope::Day => None,
            Scope::Week => Some(Scope::Day),
            Scope::Month => Some(Scope::Week),
            Scope::Quarter => Some(Scope::Month),
        }
    }
}

fn check_lens(s: &str) -> Result<&str> {
    match s {
        "both" | "work" | "life" => Ok(s),
        _ => Err(msg(format!("bad lens '{s}'"))),
    }
}

fn lens_label(lens: &str) -> &'static str {
    match lens {
        "work" => "Work",
        "life" => "Life",
        _ => "Both",
    }
}

/// The first of the month `n` months after `d`'s.
fn add_months(d: Date, n: i32) -> Date {
    d.first_of_month() + n.months()
}

/// First day of the period containing `d`.
fn period_start(scope: Scope, d: Date) -> Date {
    match scope {
        Scope::Day => d,
        Scope::Week => d - (d.weekday().to_monday_zero_offset() as i64).days(),
        Scope::Month => d.first_of_month(),
        Scope::Quarter => jiff::civil::date(d.year(), (d.month() - 1) / 3 * 3 + 1, 1),
    }
}

/// First day after the period starting at `s`.
fn period_end(scope: Scope, s: Date) -> Date {
    match scope {
        Scope::Day => s + (1).days(),
        Scope::Week => s + (7).days(),
        Scope::Month => add_months(s, 1),
        Scope::Quarter => add_months(s, 3),
    }
}

fn period_prev(scope: Scope, s: Date) -> Date {
    match scope {
        Scope::Day => s - (1).days(),
        Scope::Week => s - (7).days(),
        Scope::Month => add_months(s, -1),
        Scope::Quarter => add_months(s, -3),
    }
}

fn quarter(d: Date) -> i8 {
    (d.month() - 1) / 3 + 1
}

fn note_path(scope: Scope, s: Date, lens: &str) -> String {
    let file = match scope {
        Scope::Day => s.strftime("%Y-%m-%d").to_string(),
        Scope::Week => format!("{}-W{:02}", s.iso_week_date().year(), s.iso_week_date().week()),
        Scope::Month => s.strftime("%Y-%m").to_string(),
        Scope::Quarter => format!("{}-Q{}", s.year(), quarter(s)),
    };
    // Reflect is work-only for now: work notes sit directly under
    // Reflections/<Level>/. Other lenses (kept for later) get their own folder.
    if lens != "work" {
        format!("Reflections/{}/{}/{file}.md", lens_label(lens), scope.folder())
    } else {
        format!("Reflections/{}/{file}.md", scope.folder())
    }
}

fn titles(scope: Scope, s: Date) -> (String, String) {
    let q = quarter(s);
    match scope {
        Scope::Day => (
            s.strftime("%A %-d %B").to_string(),
            format!("in Week {} · {} · Q{q}", s.iso_week_date().week(), s.strftime("%B")),
        ),
        Scope::Week => {
            let fri = s + (4).days();
            (
                format!("Week {} · {} – {}", s.iso_week_date().week(), s.strftime("%-d %b"), fri.strftime("%-d %b")),
                format!("in {} · Q{q}", s.strftime("%B")),
            )
        }
        Scope::Month => (s.strftime("%B %Y").to_string(), format!("in Q{q} · {}", s.year())),
        Scope::Quarter => {
            let last = add_months(s, 2);
            (format!("Q{q} {} · {} – {}", s.year(), s.strftime("%b"), last.strftime("%b")), format!("in {}", s.year()))
        }
    }
}

fn template(scope: Scope) -> &'static str {
    match scope {
        Scope::Day => "",
        Scope::Week => "## Themes\n\n## What I want more of\n\n## What I want less of\n\n## Next week, three things\n1. \n2. \n3. \n",
        Scope::Month => "## Themes\n\n## What moved\n\n## What I want to change\n\n## Next month, three things\n1. \n2. \n3. \n",
        Scope::Quarter => "## Themes\n\n## What I'm glad about\n\n## What I want to change\n\n## Next quarter, three things\n1. \n2. \n3. \n",
    }
}

/// One line for the level above: the first line of writing (headings and
/// template numbering skipped), cut at the first sentence if it's long.
pub fn summarize(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !is_blank_list(l))?;
    let line = line.trim_start_matches(['-', '*', ' ']).trim();
    if line.chars().count() <= 80 {
        return Some(line.to_string());
    }
    match line.find(". ") {
        Some(i) if i < 100 => Some(line[..=i].to_string()),
        _ => Some(format!("{}…", line.chars().take(79).collect::<String>())),
    }
}

fn is_blank_list(l: &str) -> bool {
    let rest = l.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.len() < l.len() && matches!(rest.trim(), "." | ")")
        || matches!(l, "-" | "*" | "- [ ]")
}

// ---------------------------------------------------------------- events

struct Ev {
    start: DateTime,
    end: DateTime,
    title: Option<String>,
    category: String,
    life: bool,
    topic: Option<String>,
    actual: Option<(DateTime, DateTime)>,
}

impl Ev {
    fn minutes(&self) -> i64 {
        self.end.duration_since(self.start).as_mins().max(0)
    }
    fn is_meeting(&self) -> bool {
        !self.life && self.category != "self"
    }
}

fn keep(ev: &Ev, lens: &str) -> bool {
    match lens {
        "work" => !ev.life,
        "life" => ev.life,
        _ => true,
    }
}

/// Events starting on the calendar days `[from, to)` here, with their times
/// as wall-clock times here (Reflect thinks in days and minutes of the day).
fn events(r: &dyn Reads, clock: &Clock, from: Date, to: Date) -> Result<Vec<Ev>> {
    Ok(r.reflect()
        .events(clock.day_range(from).0, clock.day_range(to).0)?
        .into_iter()
        .map(|e| {
            let actual = e.raw_json.as_deref().and_then(|raw| {
                let v: Value = serde_json::from_str(raw).ok()?;
                Some((clock.local(parse_ts(v.get("actual_start")?.as_str()?)?), clock.local(parse_ts(v.get("actual_end")?.as_str()?)?)))
            });
            Ev {
                start: clock.local(e.start),
                end: clock.local(e.end),
                // Rule 3: Life blocks never carry a title or topic.
                title: if e.life { None } else { e.title },
                topic: if e.life { None } else { e.topic },
                category: e.category,
                life: e.life,
                actual: if e.life { None } else { actual },
            }
        })
        .collect())
}

/// Merged busy intervals (minutes of day) for a single day's events.
fn busy(evs: &[Ev]) -> Vec<(i64, i64)> {
    let mut iv: Vec<(i64, i64)> = evs.iter().map(|e| (minute_of_day(e.start), minute_of_day(e.end))).collect();
    iv.sort();
    let mut out: Vec<(i64, i64)> = vec![];
    for (s, e) in iv {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Free stretches between `from` and `to` (minutes of day).
fn gaps(busy: &[(i64, i64)], from: i64, to: i64) -> Vec<(i64, i64)> {
    let mut t = from;
    let mut out = vec![];
    for &(s, e) in busy {
        if e <= t {
            continue;
        }
        if s >= to {
            break;
        }
        if s > t {
            out.push((t, s));
        }
        t = t.max(e);
    }
    if t < to {
        out.push((t, to));
    }
    out
}

#[derive(Serialize, Default)]
struct CatMin {
    category: String,
    minutes: i64,
}

fn cat_totals(evs: &[&Ev]) -> Vec<CatMin> {
    let mut out: Vec<CatMin> = vec![];
    for e in evs {
        match out.iter_mut().find(|c| c.category == e.category) {
            Some(c) => c.minutes += e.minutes(),
            None => out.push(CatMin { category: e.category.clone(), minutes: e.minutes() }),
        }
    }
    // Stable order: work first, then the rest as they appear.
    let rank = |c: &str| match c {
        "work" => 0,
        "jobs" => 1,
        "life" => 2,
        "self" => 3,
        _ => 4,
    };
    out.sort_by_key(|c| rank(&c.category));
    out
}

// ---------------------------------------------------------------- get

#[derive(Serialize)]
struct Bar {
    start_min: i64,
    end_min: i64,
    category: String,
    /// Part of a meeting that ran past its scheduled end (did bar only).
    over: bool,
}

#[derive(Serialize)]
struct Tile {
    value: String,
    label: String,
}

#[derive(Serialize)]
struct Where {
    label: String,
    category: String,
    minutes: i64,
    dur: String,
}

#[derive(Serialize)]
struct Decision {
    title: String,
    topic: Option<String>,
}

#[derive(Serialize)]
struct CarryRow {
    item_id: i64,
    period: String,
    /// do | research | delegated
    glyph: String,
    title: String,
    owner: Option<String>,
    action: Option<String>,
    /// Outbox state for a scheduled hold (pending/sent/confirmed/failed).
    outbox: Option<String>,
}

#[derive(Serialize)]
struct Child {
    label: String,
    period: String,
    summary: Option<String>,
    has_entry: bool,
    cats: Vec<CatMin>,
}

fn get(app: &App, args: &Value) -> Result<Value> {
    let today = app.clock.today();
    let scope = Scope::parse(args.get("scope").and_then(Value::as_str).unwrap_or("day"))?;
    let lens = check_lens(args.get("lens").and_then(Value::as_str).unwrap_or("work"))?.to_string();
    let d = match args.get("period").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => parse_date(p)?,
        _ => today,
    };
    let s = period_start(scope, d);
    let e = period_end(scope, s);
    let (title, context) = titles(scope, s);
    let settings = app.settings();

    let (brief, children, carry) = app.store().read(|r| {
        let brief = if scope == Scope::Day { Some(day_brief(r, &app.clock, &settings, s, &lens)?) } else { None };
        let children = match scope.child() {
            Some(child) => Some(rollup(r, &app.clock, child, s, e, &lens)?),
            None => None,
        };
        let carry = if lens == "life" { vec![] } else { carry_rows(r, scope, s, e)? };
        Ok((brief, children, carry))
    })?;
    // The vault after the snapshot: the notes, and each child period's own note.
    let children = children.map(|cs| cs.into_iter().filter_map(|c| c.with_note(&app.vault, &lens)).collect::<Vec<_>>());
    let path = note_path(scope, s, &lens);
    let abs = app.vault.join(&path);
    let disk = std::fs::read_to_string(&abs).ok();
    let note = json!({
        "path": path,
        "abs_path": abs.to_string_lossy(),
        "text": disk.clone().unwrap_or_else(|| template(scope).to_string()),
        "base": disk,
    });

    Ok(json!({
        "scope": scope.key(),
        "lens": lens,
        "period": s.strftime("%Y-%m-%d").to_string(),
        "today": today.strftime("%Y-%m-%d").to_string(),
        "is_current": today >= s && today < e,
        "prev": period_prev(scope, s).strftime("%Y-%m-%d").to_string(),
        "next": e.strftime("%Y-%m-%d").to_string(),
        "title": title,
        "context": context,
        "note": note,
        "brief": brief,
        "children": children,
        "carry": carry,
    }))
}

fn day_brief(r: &dyn Reads, clock: &Clock, settings: &Settings, day: Date, lens: &str) -> Result<Value> {
    let now = clock.local_now();
    let (day_from, day_to) = clock.day_range(day);
    let all = events(r, clock, day, day + (1).days())?;
    let evs: Vec<&Ev> = all.iter().filter(|e| keep(e, lens)).collect();
    let (h0, h1) = (settings.day_start_hour as i64 * 60, settings.day_end_hour as i64 * 60);
    // Free time counts every block as busy (time math on Life is allowed).
    let free = gaps(&busy(&all), h0, h1);
    let free_total: i64 = free.iter().map(|g| g.1 - g.0).sum();
    let longest = free.iter().map(|g| g.1 - g.0).max().unwrap_or(0);
    let decided: Vec<Decision> = if lens == "life" {
        vec![]
    } else {
        r.reflect().decisions(day_from, day_to)?.into_iter().map(|(title, topic)| Decision { title, topic }).collect()
    };

    let tiles = if lens == "life" {
        let life: Vec<&&Ev> = evs.iter().filter(|e| e.life).collect();
        vec![
            Tile { value: dur_label(life.iter().map(|e| e.minutes()).sum()), label: "Life · busy".into() },
            Tile { value: life.len().to_string(), label: "Life blocks".into() },
            Tile { value: dur_label(longest), label: format!("longest of {} free", dur_label(free_total)) },
            Tile { value: dur_label(free_total), label: format!("free {:02}–{:02}", h0 / 60, h1 / 60) },
        ]
    } else {
        let meetings: Vec<&&Ev> = evs.iter().filter(|e| e.is_meeting()).collect();
        let decisions = decided.len();
        let (jotted, held) = r.notes().page_counts(day)?;
        vec![
            Tile {
                value: meetings.len().to_string(),
                label: format!("meetings · {}", dur_label(meetings.iter().map(|e| e.minutes()).sum())),
            },
            Tile { value: decisions.to_string(), label: if decisions == 1 { "decision" } else { "decisions" }.into() },
            Tile { value: dur_label(longest), label: format!("longest of {} free", dur_label(free_total)) },
            Tile {
                value: jotted.to_string(),
                label: if held > 0 { format!("jotted, {held} held") } else { "jotted in Notes".into() },
            },
        ]
    };

    let plan: Vec<Bar> = evs
        .iter()
        .map(|e| Bar { start_min: minute_of_day(e.start), end_min: minute_of_day(e.end), category: e.category.clone(), over: false })
        .collect();

    let mut did = vec![];
    let now_min = if day == now.date() { minute_of_day(now) } else if day < now.date() { 24 * 60 } else { 0 };
    for e in &evs {
        let (a, b) = e.actual.map(|(a, b)| (minute_of_day(a), minute_of_day(b))).unwrap_or((minute_of_day(e.start), minute_of_day(e.end)));
        if a >= now_min {
            continue;
        }
        let b = b.min(now_min);
        let sched_end = minute_of_day(e.end);
        if b > sched_end {
            did.push(Bar { start_min: a, end_min: sched_end, category: e.category.clone(), over: false });
            did.push(Bar { start_min: sched_end, end_min: b, category: e.category.clone(), over: true });
        } else {
            did.push(Bar { start_min: a, end_min: b, category: e.category.clone(), over: false });
        }
    }

    let mut where_: Vec<Where> = vec![];
    for e in &evs {
        let (label, cat) = if e.life {
            ("Life · busy".to_string(), "life".to_string())
        } else if e.category == "self" {
            ("Self-holds".to_string(), "self".to_string())
        } else {
            (e.topic.clone().unwrap_or_else(|| "Other meetings".into()), e.category.clone())
        };
        match where_.iter_mut().find(|w| w.label == label) {
            Some(w) => w.minutes += e.minutes(),
            None => where_.push(Where { label, category: cat, minutes: e.minutes(), dur: String::new() }),
        }
    }
    where_.sort_by(|a, b| b.minutes.cmp(&a.minutes));
    for w in &mut where_ {
        w.dur = dur_label(w.minutes);
    }

    Ok(json!({
        "tiles": tiles,
        "range": { "start_min": h0, "end_min": h1 },
        "plan": plan,
        "did": did,
        "did_note": "did: calendar with actual join/leave times where known (sample on two meetings), up to now",
        "where": where_,
        "decided": decided,
    }))
}

/// A child period in a rollup, before its note is looked at.
struct ChildRow {
    scope: Scope,
    start: Date,
    label: String,
    weekend: bool,
    events: bool,
    /// The stored reflection, if any: Some(summary).
    stored: Option<Option<String>>,
    cats: Vec<CatMin>,
}

impl ChildRow {
    /// The note is the truth (edited in Obsidian, or saved through it); the
    /// stored summary is for when the file isn't there. Weekends only show up
    /// when something happened.
    fn with_note(self, vault: &Path, lens: &str) -> Option<Child> {
        let (summary, has_entry) = match std::fs::read_to_string(vault.join(note_path(self.scope, self.start, lens))) {
            Ok(text) => (summarize(&text), true),
            Err(_) => match self.stored {
                Some(summary) => (summary, true),
                None => (None, false),
            },
        };
        (!self.weekend || has_entry || self.events).then(|| Child {
            label: self.label,
            period: self.start.strftime("%Y-%m-%d").to_string(),
            summary,
            has_entry,
            cats: self.cats,
        })
    }
}

fn rollup(r: &dyn Reads, clock: &Clock, child: Scope, s: Date, e: Date, lens: &str) -> Result<Vec<ChildRow>> {
    let all = events(r, clock, period_start(child, s), period_end(child, period_start(child, e - (1).days())))?;
    let mut out = vec![];
    let mut c = period_start(child, s);
    while c < e {
        let ce = period_end(child, c);
        let evs: Vec<&Ev> = all.iter().filter(|x| keep(x, lens) && x.start.date() >= c && x.start.date() < ce).collect();
        let label = match child {
            Scope::Day => c.strftime("%a").to_string(),
            Scope::Week => format!("W{:02}", c.iso_week_date().week()),
            _ => c.strftime("%b").to_string(),
        };
        out.push(ChildRow {
            scope: child,
            start: c,
            label,
            weekend: child == Scope::Day && c.weekday().to_monday_zero_offset() >= 5,
            events: !evs.is_empty(),
            stored: r.reflect().summary(child.key(), c, lens)?,
            cats: cat_totals(&evs),
        });
        c = ce;
    }
    Ok(out)
}

/// Day: that day's carry list (acted rows included, faded in the UI).
/// Week and up: items still open at the end of their latest carry in range.
fn carry_rows(r: &dyn Reads, scope: Scope, s: Date, e: Date) -> Result<Vec<CarryRow>> {
    let mut rows = vec![];
    for c in r.reflect().carry(s, e, scope != Scope::Day)? {
        let glyph = serde_json::from_str::<Value>(&c.raw_json)
            .ok()
            .and_then(|v| v.get("glyph").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| "do".into());
        let outbox = if c.action.as_deref() == Some("schedule") { r.reflect().hold_state(c.item_id, c.period)? } else { None };
        rows.push(CarryRow {
            item_id: c.item_id,
            period: c.period.to_string(),
            title: c.title,
            owner: c.owner,
            glyph,
            action: c.action,
            outbox,
        });
    }
    Ok(rows)
}

// ---------------------------------------------------------------- save

fn save(app: &App, args: &Value) -> Result<Value> {
    let scope = Scope::parse(&arg_str(args, "scope")?)?;
    let lens = check_lens(&arg_str(args, "lens")?)?.to_string();
    let s = period_start(scope, parse_date(&arg_str(args, "period")?)?);
    let text = arg_str(args, "text")?;
    let base = args.get("base").and_then(Value::as_str).map(str::to_owned);
    let path = note_path(scope, s, &lens);

    // Never clobber: write only if the note still holds what the UI loaded.
    // With an obsidian source this queues `obsidian.replace` (written through
    // Obsidian, checked there); the summary then comes from the file once it
    // lands (`reflection_summary`). Without one (sample data) it's a direct write.
    match save::save_note(app, &path, base.as_deref(), &text)? {
        save::Saved::Conflict(disk) => return Ok(json!({ "conflict": true, "disk": disk })),
        save::Saved::Queued(id) => return Ok(json!({ "queued": id, "base": text })),
        save::Saved::Written => {}
    }
    let summary = summarize(&text);
    app.store().write(|uow| uow.reflections().save_summary(scope.key(), s, &lens, &path, summary.as_deref()))?;
    Ok(json!({ "ok": true, "base": text, "summary": summary }))
}

// ---------------------------------------------------------------- carry

fn carry(uow: &dyn UnitOfWork, today: Date, item_id: i64, period: &str, action: &str) -> Result<Value> {
    let p = parse_date(period)?;
    let Some(current) = uow.reflections().carry_action(item_id, p)? else {
        return Err(msg("not on the carry list"));
    };
    // Tomorrow is relative to the later of the carry day and today.
    let target = p.max(today) + (1).days();
    match action {
        "tomorrow" | "schedule" | "let_go" => {
            if current.is_some() {
                return Err(msg("already decided; undo first"));
            }
            uow.reflections().set_carry_action(item_id, p, Some(action))?;
            match action {
                "tomorrow" => uow.reflections().carry_to(item_id, target)?,
                "schedule" => {
                    // A calendar hold goes through the outbox; the item itself stays open.
                    let title = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?.title;
                    let gcal = uow.reads().sources().of_kind("gcal")?.into_iter().next().ok_or_else(|| msg("no Google Calendar source for the hold"))?;
                    let payload = json!({ "item_id": item_id, "title": title, "carried_from": p.to_string() });
                    uow.outbox().enqueue(gcal.id, "gcal.hold", &payload.to_string(), None)?;
                }
                _ => {} // let go: nothing is deleted; it just leaves the list.
            }
        }
        "undo" => {
            match current.as_deref() {
                None => return Ok(json!({ "ok": true })),
                Some("tomorrow") => uow.reflections().uncarry(item_id, target)?,
                Some("schedule") => {
                    if uow.outbox().cancel_pending_hold(item_id, p)? == 0 {
                        return Err(msg("the hold was already sent; remove it in the calendar"));
                    }
                }
                _ => {}
            }
            uow.reflections().set_carry_action(item_id, p, None)?;
        }
        _ => return Err(msg(format!("bad carry action '{action}'"))),
    }
    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------- nudge (9d)

/// The nudge looks for the first real gap after this time (local).
const NUDGE_AFTER_MIN: i64 = 16 * 60 + 30;
const NUDGE_GAP_MIN: i64 = 15;
const SNOOZE_MIN: i64 = 10;

#[derive(Serialize, Deserialize, Default)]
struct NudgeState {
    date: String,
    /// now | not_today | dismissed. Only today's answer is kept; nothing is tallied.
    answer: Option<String>,
    snooze_until: Option<String>,
    notified: bool,
}

fn nudge_file(app: &App) -> std::path::PathBuf {
    app.data_dir.join("reflect-nudge.json")
}

fn load_nudge(app: &App, today: &str) -> NudgeState {
    std::fs::read_to_string(nudge_file(app))
        .ok()
        .and_then(|t| serde_json::from_str::<NudgeState>(&t).ok())
        .filter(|s| s.date == today)
        .unwrap_or_else(|| NudgeState { date: today.into(), ..Default::default() })
}

fn store_nudge(app: &App, st: &NudgeState) -> Result<()> {
    std::fs::write(nudge_file(app), serde_json::to_string(st)?)?;
    Ok(())
}

/// The gap the nudge belongs in: the first stretch of ≥15 free minutes after
/// 16:30 that hasn't already ended. `None` end = nothing else on the calendar.
fn nudge_gap(busy: &[(i64, i64)], now_min: i64) -> Option<(i64, Option<i64>)> {
    for (s, e) in gaps(busy, NUDGE_AFTER_MIN, 24 * 60) {
        let open_ended = !busy.iter().any(|b| b.0 >= e);
        if e - s >= NUDGE_GAP_MIN && (open_ended || e > now_min) {
            return Some((s, if open_ended { None } else { Some(e) }));
        }
    }
    None
}

fn nudge(app: &App) -> Result<Value> {
    let now = app.clock.local_now();
    let today = now.date();
    let ds = today.strftime("%Y-%m-%d").to_string();
    let now_min = minute_of_day(now);
    let mut st = load_nudge(app, &ds);
    let (evs, open) = app.store().read(|r| Ok((events(r, &app.clock, today, today + (1).days())?, r.reflect().open_carry(today)?)))?;

    let gap = nudge_gap(&busy(&evs), now_min);
    // A snooze is an instant (it can run past midnight).
    let snoozed_until = st.snooze_until.as_deref().and_then(Ts::parse);
    let show = st.answer.is_none()
        && match snoozed_until {
            Some(t) => app.clock.now() >= t,
            None => gap.map_or(false, |(s, e)| now_min >= s && e.map_or(true, |e| now_min < e)),
        };

    let until = gap.and_then(|(_, e)| e).and_then(|e| {
        let ev = evs.iter().find(|x| minute_of_day(x.start) == e)?;
        let what = if ev.life { "Life".to_string() } else { ev.title.clone().unwrap_or_else(|| "a meeting".into()) };
        Some(format!("No meetings until {what} at {}", ev.start.strftime("%H:%M")))
    });
    let things = match open {
        0 => None,
        1 => Some("1 thing to carry forward".to_string()),
        n => Some(format!("{n} things to carry forward")),
    };
    let line = [until.unwrap_or_else(|| "Nothing else on the calendar".into())]
        .into_iter()
        .chain(things)
        .collect::<Vec<_>>()
        .join(" · ");

    if show && !st.notified {
        // The real macOS notification and tray dot (stubs on Linux).
        app.platform.notify_persistent("Wrap up the day?", &line);
        app.platform.set_tray_dot(true);
        st.notified = true;
        store_nudge(app, &st)?;
    }

    Ok(json!({
        "show": show,
        "at": now.strftime("%H:%M").to_string(),
        "title": "Wrap up the day?",
        "line": line,
        "carry_count": open,
        "fires_at": gap.map(|(s, _)| format!("{:02}:{:02}", s / 60, s % 60)),
        "answer": st.answer,
    }))
}

fn nudge_answer(app: &App, answer: &str) -> Result<Value> {
    let ds = app.clock.today().to_string();
    let mut st = load_nudge(app, &ds);
    match answer {
        "snooze" => {
            st.snooze_until = Some((app.clock.now() + jiff::SignedDuration::from_mins(SNOOZE_MIN)).to_db());
            st.notified = false;
        }
        "now" | "not_today" | "dismissed" => {
            st.answer = Some(answer.into());
            st.snooze_until = None;
        }
        _ => return Err(msg(format!("bad answer '{answer}'"))),
    }
    store_nudge(app, &st)?;
    app.platform.set_tray_dot(false);
    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------- seed

pub fn seed(ctx: &Ctx, vault: &Path) -> anyhow::Result<()> {
    let sample = ctx.sample();
    let today = ctx.today;
    let ds = |d: Date| d.strftime("%Y-%m-%d").to_string();
    let monday = period_start(Scope::Week, today);

    // "did" sample: actual join/leave times on two of today's meetings.
    for (title, a, b) in [("Eng leads", "09:00", "10:10"), ("1:1 Marco", "10:30", "10:45"), ("Acme call", "16:15", "16:55")] {
        let raw = json!({ "actual_start": ctx.at(0, a), "actual_end": ctx.at(0, b) }).to_string();
        sample.patch_item(ctx.meeting_id(0, title), &ItemPatch { raw_json: Some(&raw), ..Default::default() })?;
    }

    // Decisions recorded today (Obsidian meeting notes).
    for (title, topic, at) in [
        ("Headcount: 1 Platform + 1 Data", "Q4 headcount", "11:40"),
        ("Acme: backoff fix ships behind a flag", "Acme outage", "15:05"),
    ] {
        let id = ctx.item(ItemSeed { source: "obsidian", kind: "decision", title: title.into(), occurred_at: ctx.at(0, at), ..Default::default() });
        ctx.link(id, topic, "confirmed");
    }

    // Earlier this week: a few meetings per day, for the category bars.
    let week: [&[(&str, &str, &str, &str)]; 5] = [
        &[("09:00", "10:00", "Eng leads", "work"), ("10:30", "12:30", "Acme incident review", "work"), ("12:30", "13:00", "Lunch", "self"), ("14:00", "16:00", "Planning", "work")],
        &[("09:30", "11:00", "Datadog prep", "work"), ("12:00", "12:45", "Lunch", "self"), ("13:00", "14:00", "1:1 Priya", "work")],
        &[("09:00", "10:30", "Staff loop", "work"), ("11:00", "12:30", "Vendor sync", "work"), ("12:30", "13:15", "Lunch", "self"), ("15:00", "15:45", "1:1 Jordan", "work")],
        &[("10:30", "11:30", "Standup + triage", "work"), ("12:00", "13:00", "Lunch", "self"), ("13:00", "14:00", "Walk", "self"), ("14:30", "16:00", "Hiring loop", "work"), ("16:00", "16:30", "Career chat", "jobs")],
        &[("09:00", "11:00", "Eng leads", "work"), ("12:00", "13:00", "Lunch", "self"), ("14:00", "16:00", "Offsite prep", "work")],
    ];
    let life: [&[(&str, &str)]; 5] = [
        &[("17:00", "18:00")],
        &[("15:00", "17:30")],
        &[("16:30", "17:30")],
        &[("17:30", "18:30")],
        &[],
    ];
    let one_liners = [
        ("Headcount decided; Acme call took the open hour", "Mostly reacting. The incident review was useful though."),
        ("Datadog prep; short day", "Left early. Numbers are in the sheet."),
        ("Counter-offer sent", "Kim agreed on the walk-away number."),
        ("Protected morning worked", "No meetings before 10:30 and the doc moved."),
    ];
    let mut earlier = vec![];
    for i in 0..5 {
        let d = monday + (i as i64).days();
        if d >= today {
            break;
        }
        earlier.push(d);
        let off = (d - today).get_days() as i64;
        for (a, b, title, cat) in week[i] {
            ctx.meeting(off, a, b, title, cat, "", false);
        }
        for (a, b) in life[i] {
            ctx.life_block(off, a, b);
        }
        if let Some((line, more)) = one_liners.get(i) {
            write_seed_note(sample, vault, Scope::Day, d, &format!("{line}\n\n{more}\n"))?;
        }
    }

    // Today's note (9b).
    write_seed_note(
        sample,
        vault,
        Scope::Day,
        today,
        "Headcount call went 1 Platform + 1 Data. Felt right once Lee's numbers were in the room.\n\n\
         The Acme call took my only open hour. Fine, but the strategy doc slipped again. Protect tomorrow morning.\n",
    )?;

    // The week note (9c).
    write_seed_note(
        sample,
        vault,
        Scope::Week,
        monday,
        "## Themes\nAcme outage ate the start of the week; headcount got decided; Datadog is close.\n\n\
         ## What I want more of\nMornings like Thursday: no meetings before 10:30.\n\n\
         ## What I want less of\nChecking Slack between back-to-backs.\n\n\
         ## Next week, three things\n1. \n2. \n3. \n",
    )?;

    // Carry forward: today's list, plus what trailed through the week.
    let task = |title: &str, owner: Option<&str>, days: i64| {
        ctx.item(ItemSeed {
            source: "obsidian",
            kind: "task",
            title: title.into(),
            occurred_at: ctx.at(days, "09:00"),
            owner: owner.map(Into::into),
            ..Default::default()
        })
    };
    let set_glyph = |id: i64, glyph: &str| sample.patch_item(id, &ItemPatch { raw_json: Some(&json!({ "glyph": glyph }).to_string()), ..Default::default() });
    let first = earlier.first().copied().unwrap_or(today);
    let items = [
        (task("Send Marco feedback on the backoff PR", None, 0), "do"),
        (task("attrition numbers", Some("Lee"), 0), "delegated"),
        (task("Platform on-call models", None, (first - today).get_days() as i64), "research"),
        (task("Q4 strategy doc, first draft", None, (first - today).get_days() as i64), "do"),
    ];
    for (id, glyph) in items {
        set_glyph(id, glyph)?;
    }
    // The strategy doc and on-call models trailed day to day through the week.
    for d in &earlier {
        for (id, _) in &items[2..] {
            sample.carry(*id, &ds(*d), Some("tomorrow"))?;
        }
    }
    if let Some(last) = earlier.last() {
        let jordan = task("node-7 owner", Some("Jordan"), (*last - today).get_days() as i64);
        set_glyph(jordan, "delegated")?;
        sample.carry(jordan, &ds(*last), None)?;
    }
    for (id, _) in &items {
        sample.carry(*id, &ds(today), None)?;
    }
    Ok(())
}

fn write_seed_note(sample: &dyn SampleData, vault: &Path, scope: Scope, s: Date, text: &str) -> anyhow::Result<()> {
    let path = note_path(scope, s, "work");
    let abs = vault.join(&path);
    if !abs.exists() {
        if let Some(dir) = abs.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&abs, text)?;
    }
    sample.reflection(scope.key(), &s.strftime("%Y-%m-%d").to_string(), &path, summarize(text).as_deref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use super::*;
    use crate::settings::Settings;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.fake_now = "10:52".into();
        let conn = crate::db::open_in_memory().unwrap();
        let app = App::from_parts(conn, settings, dir.path()).unwrap();
        (app, dir)
    }

    fn today(app: &App) -> String {
        app.clock.today().to_string()
    }

    #[test]
    fn sample_meetings_get_their_join_times_east_of_utc() {
        // 09:00 in Sydney is the previous day in UTC: the meeting is still today's.
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let clock = crate::clock::Clock::fixed(crate::time::Ts::parse("2026-10-02T23:52:00Z").unwrap(), jiff::tz::TimeZone::get("Australia/Sydney").unwrap());
        crate::seed::seed(&crate::store::over(&conn), &clock, dir.path()).unwrap();
        let raw: String = conn
            .query_row("SELECT i.raw_json FROM events e JOIN items i ON i.id = e.item_id WHERE e.title = 'Eng leads' AND e.start = ?1", [clock.from_local(clock.today().at(9, 0, 0, 0))], |r| r.get(0))
            .unwrap();
        assert!(raw.contains("actual_start"), "{raw}");
    }

    #[test]
    fn summary_is_first_line_of_writing() {
        assert_eq!(summarize("## Themes\nAcme ate the week.\n"), Some("Acme ate the week.".into()));
        assert_eq!(summarize("## Next\n1. \n2. \n"), None);
        let long = "Headcount call went 1 Platform + 1 Data. Felt right once Lee's numbers were in the room.";
        assert_eq!(summarize(long), Some("Headcount call went 1 Platform + 1 Data.".into()));
    }

    #[test]
    fn periods_and_paths() {
        let d = jiff::civil::date(2026, 9, 30);
        let w = period_start(Scope::Week, d);
        assert_eq!(w, jiff::civil::date(2026, 9, 28));
        assert_eq!(note_path(Scope::Week, w, "work"), "Reflections/Weekly/2026-W40.md");
        assert_eq!(note_path(Scope::Day, w, "life"), "Reflections/Life/Daily/2026-09-28.md");
        let q = period_start(Scope::Quarter, d);
        assert_eq!(note_path(Scope::Quarter, q, "work"), "Reflections/Quarterly/2026-Q3.md");
        assert_eq!(period_end(Scope::Quarter, q), jiff::civil::date(2026, 10, 1));
        assert_eq!(titles(Scope::Day, w).0, "Monday 28 September");
        assert_eq!(titles(Scope::Week, w).0, "Week 40 · 28 Sep – 2 Oct");
    }

    #[test]
    fn save_never_clobbers() {
        let (app, _d) = app();
        let t = today(&app);
        let got = get(&app, &json!({ "scope": "day", "period": t, "lens": "both" })).unwrap();
        assert!(got["note"]["base"].is_null());
        let r = save(&app, &json!({ "scope": "day", "period": t, "lens": "both", "text": "Good day.\n", "base": null })).unwrap();
        assert_eq!(r["ok"], true);
        // Saving again from a stale "absent" base is refused.
        let r = save(&app, &json!({ "scope": "day", "period": t, "lens": "both", "text": "x", "base": null })).unwrap();
        assert_eq!(r["conflict"], true);
        // Someone edits the file in Obsidian: our save with the old base is refused.
        let abs = app.vault.join(note_path(Scope::Day, parse_date(&t).unwrap(), "both"));
        std::fs::write(&abs, "Edited in Obsidian\n").unwrap();
        let r = save(&app, &json!({ "scope": "day", "period": t, "lens": "both", "text": "mine", "base": "Good day.\n" })).unwrap();
        assert_eq!(r["conflict"], true);
        assert_eq!(std::fs::read_to_string(&abs).unwrap(), "Edited in Obsidian\n");
        // With the current base it lands and the summary rolls up.
        let r = save(&app, &json!({ "scope": "day", "period": t, "lens": "both", "text": "Calm.\nmore", "base": "Edited in Obsidian\n" })).unwrap();
        assert_eq!(r["summary"], "Calm.");
    }

    #[test]
    fn save_goes_through_obsidian_when_a_source_is_configured() {
        use crate::sync::http::FakeHttp;
        let (app, _d) = crate::sync::connectors::obsidian::save::tests::app_with_source("obsidian");
        let t = today(&app);
        let got = get(&app, &json!({ "scope": "day", "period": t, "lens": "work" })).unwrap();
        let base = got["note"]["base"].clone();
        let r = save(&app, &json!({ "scope": "day", "period": t, "lens": "work", "text": "Calm day.\n", "base": base })).unwrap();
        let id = r["queued"].as_i64().unwrap();
        assert_eq!(r["base"], "Calm day.\n");
        // Nothing on disk yet: that's Obsidian's job.
        let path = note_path(Scope::Day, parse_date(&t).unwrap(), "work");
        assert_ne!(std::fs::read_to_string(app.vault.join(&path)).ok().as_deref(), Some("Calm day.\n"));

        let http = FakeHttp::default();
        match base.as_str() {
            Some(b) => http.on("GET", "/vault/Reflections/", 200, json!({ "content": b, "stat": { "mtime": 1 } }).to_string()),
            None => http.on("GET", "/vault/Reflections/", 404, "{}"),
        };
        http.on("PUT", "/vault/Reflections/", 204, "");
        crate::outbox::process_one(&app, &http).unwrap();
        assert_eq!(call(&app, "save_status", &json!({ "outbox_id": id })).unwrap()["state"], "saved");
        // Obsidian writes the file; the rollup summary then comes from it.
        std::fs::write(app.vault.join(&path), "Calm day.\n").unwrap();
        let week = get(&app, &json!({ "scope": "week", "lens": "work", "period": t })).unwrap();
        let day = week["children"].as_array().unwrap().iter().find(|c| c["period"] == t.as_str()).unwrap().clone();
        assert_eq!((day["summary"].as_str(), day["has_entry"].as_bool()), (Some("Calm day."), Some(true)));
    }

    #[test]
    fn carry_actions() {
        let (app, _d) = app();
        let t = today(&app);
        let got = get(&app, &json!({ "scope": "day", "lens": "work" })).unwrap();
        let rows = got["carry"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        let id = |i: usize| rows[i]["item_id"].as_i64().unwrap();

        call(&app, "carry", &json!({ "item_id": id(0), "period": t, "action": "tomorrow" })).unwrap();
        let tomorrow = (app.clock.today() + (1).days()).to_string();
        let n: i64 = app.db().query_row("SELECT count(*) FROM carry WHERE item_id = ?1 AND period = ?2", params![id(0), tomorrow], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert!(call(&app, "carry", &json!({ "item_id": id(0), "period": t, "action": "let_go" })).is_err(), "must undo first");
        call(&app, "carry", &json!({ "item_id": id(0), "period": t, "action": "undo" })).unwrap();
        let n: i64 = app.db().query_row("SELECT count(*) FROM carry WHERE item_id = ?1 AND period = ?2", params![id(0), tomorrow], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);

        call(&app, "carry", &json!({ "item_id": id(1), "period": t, "action": "schedule" })).unwrap();
        let state: String = app.db().query_row("SELECT state FROM outbox WHERE op = 'gcal.hold'", [], |r| r.get(0)).unwrap();
        assert_eq!(state, "pending");
        let got = get(&app, &json!({ "scope": "day", "lens": "work" })).unwrap();
        assert_eq!(got["carry"][1]["outbox"], "pending");

        // Let go never deletes the item.
        call(&app, "carry", &json!({ "item_id": id(2), "period": t, "action": "let_go" })).unwrap();
        let n: i64 = app.db().query_row("SELECT count(*) FROM items WHERE id = ?1", [id(2)], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let week = get(&app, &json!({ "scope": "week", "lens": "work" })).unwrap();
        assert!(week["carry"].as_array().unwrap().iter().all(|r| r["item_id"].as_i64() != Some(id(2))));
    }

    #[test]
    fn life_lens_is_busy_totals_only() {
        let (app, _d) = app();
        let got = get(&app, &json!({ "scope": "day", "lens": "life" })).unwrap();
        let b = &got["brief"];
        assert!(b["plan"].as_array().unwrap().iter().all(|x| x["category"] == "life"));
        assert!(b["where"].as_array().unwrap().iter().all(|x| x["label"] == "Life · busy"));
        assert!(b["decided"].as_array().unwrap().is_empty());
        assert!(got["carry"].as_array().unwrap().is_empty());
        let s = got.to_string();
        assert!(!s.contains("Eng leads") && !s.contains("Acme"));
    }

    #[test]
    fn week_rolls_up_days() {
        let (app, _d) = app();
        let got = get(&app, &json!({ "scope": "week", "lens": "work" })).unwrap();
        let kids = got["children"].as_array().unwrap();
        assert!(kids.len() >= 5);
        // The seed fills today and the weekdays around it, so the week has a quiet weekend day.
        assert!(kids.iter().all(Value::is_object), "a quiet weekend day is left out, not sent as null: {kids:?}");
        let today_row = kids.iter().find(|k| k["period"] == today(&app)).unwrap();
        assert_eq!(today_row["summary"], "Headcount call went 1 Platform + 1 Data.");
        assert_eq!(got["note"]["path"].as_str().unwrap().starts_with("Reflections/Weekly/"), true);
    }

    #[test]
    fn nudge_gap_rules() {
        // Busy until 17:15, Life 17:30–18:00, walk 18:00–19:00.
        let busy = vec![(16 * 60 + 15, 17 * 60 + 15), (17 * 60 + 30, 19 * 60)];
        assert_eq!(nudge_gap(&busy, 17 * 60 + 20), Some((17 * 60 + 15, Some(17 * 60 + 30))));
        // Gap missed: the open evening after 19:00 is next.
        assert_eq!(nudge_gap(&busy, 17 * 60 + 40), Some((19 * 60, None)));
        // 10-minute gaps don't count.
        let busy = vec![(16 * 60, 17 * 60), (17 * 60 + 10, 18 * 60)];
        assert_eq!(nudge_gap(&busy, 17 * 60 + 5), Some((18 * 60, None)));
    }

    #[test]
    fn nudge_answers_stick_for_the_day() {
        let (app, _d) = app();
        // At 10:52 there is no nudge.
        assert_eq!(nudge(&app).unwrap()["show"], false);
        nudge_answer(&app, "not_today").unwrap();
        let v = nudge(&app).unwrap();
        assert_eq!(v["answer"], "not_today");
        assert_eq!(v["show"], false);
        assert!(nudge_answer(&app, "bogus").is_err());
    }
}

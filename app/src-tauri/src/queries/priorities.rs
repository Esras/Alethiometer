//! Priorities (app/docs/priorities.md): open todos in three orders side by
//! side, Priority, Quick wins and Neglect. Every column holds every todo.
//!
//! Scoring is Taskwarrior-style: each factor is scored 0 to 1 and multiplied
//! by its coefficient (`settings.priorities.coefficients`); the Priority score
//! is the sum. Value, Pressure and Effort group the factors for the reason
//! pills and for the other two columns. The database gives each todo's facts
//! (`PriorityViews::facts`); the curves are here, because they need the Clock
//! and the Resolver (who is "you", whose People note has which weight). Nothing
//! is stored: every read scores afresh (user, 2026-10-06).
//!
//! Not built yet: thread and topic heat (the baseline is worked out on the work
//! Mac), tag suggestions, delegated asks.
//!
//! Work only: items from personal sources never appear here (boundary rule 3).

use super::people::Resolver;
use super::todos::TodoExt;
use super::{dur_label, when_label, Pill, TopicRef};
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::ports::Reads;
use crate::domain::priorities::TodoFacts;
use crate::error::Result;
use crate::settings::{PrioritySettings, Settings};
use crate::time::{Date, Due, Ts};
use jiff::SignedDuration;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn route(name: &str) -> Option<Route> {
    match name {
        "list" => Some(Route::Read(|r, ctx, _| Ok(serde_json::to_value(list(r, ctx.clock, ctx.settings)?)?))),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Value,
    Pressure,
    Effort,
}

/// One factor of one todo: its 0-to-1 value and what it adds to the score.
#[derive(Debug, Clone)]
pub struct Factor {
    /// `due`, `sched`, `meeting`, `waiting`, `blocks`, `topic_priority`, `who_asked`, `breadth`, `estimate`, `tag:<name>`.
    pub key: String,
    pub group: Group,
    pub value: f64,
    pub points: f64,
    /// The reason pill's text.
    pub label: String,
}

// ---------------------------------------------------------------- the curves

/// Taskwarrior's due curve (`Task::urgency_due`): 0.2 two weeks or more out,
/// rising linearly to 1.0 at a week overdue.
pub fn due_curve(days_overdue: f64) -> f64 {
    if days_overdue >= 7.0 {
        1.0
    } else if days_overdue >= -14.0 {
        (days_overdue + 14.0) * 0.8 / 21.0 + 0.2
    } else {
        0.2
    }
}

/// Taskwarrior's scheduled term: 1 once the day has come (user, 2026-10-06).
pub fn sched_curve(sched: Date, today: Date) -> f64 {
    if sched <= today { 1.0 } else { 0.0 }
}

/// The next meeting on the topic: 1 within 15 minutes of its start or while it
/// runs, falling linearly to 0 at 8 hours out.
pub fn meeting_curve(minutes_to_start: f64) -> f64 {
    if minutes_to_start <= 15.0 { 1.0 } else { (1.0 - (minutes_to_start - 15.0) / (480.0 - 15.0)).max(0.0) }
}

/// Someone else spoke last: 0 right after, 1 after three days.
pub fn waiting_curve(hours: f64) -> f64 {
    (hours / 72.0).clamp(0.0, 1.0)
}

/// People and sources on the topic: half each, full at 10 people and 5 sources.
pub fn breadth_curve(people: i64, sources: i64) -> f64 {
    0.5 * (people as f64 / 10.0).min(1.0) + 0.5 * (sources as f64 / 5.0).min(1.0)
}

/// Short work scores high: 1 at 5 minutes or less, 0 at 4 hours or more, on a log scale.
pub fn estimate_curve(minutes: i64) -> f64 {
    let m = minutes.max(1) as f64;
    (1.0 - (m / 5.0).log2() / 48f64.log2()).clamp(0.0, 1.0)
}

fn days(d: SignedDuration) -> f64 {
    d.as_secs_f64() / 86_400.0
}

/// "today", "Wed", "Oct 9".
fn day_label(d: Date, clock: &Clock) -> String {
    when_label(d.at(12, 0, 0, 0), clock.local_now(), false).replace("Today", "today")
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "3h", "2d".
fn ago_label(d: SignedDuration) -> String {
    let h = d.as_secs() / 3600;
    if h < 24 { format!("{}h", h.max(1)) } else { format!("{}d", h / 24) }
}

/// Every factor of a todo, with its value and points, those at 0 included.
pub fn factors(t: &TodoFacts, p: &PrioritySettings, clock: &Clock, who: &Resolver, meeting: Option<(Ts, Ts)>) -> Vec<Factor> {
    let c = &p.coefficients;
    let now = clock.now();
    let today = clock.today();
    let mut out = vec![];
    let mut add = |key: &str, group: Group, value: f64, coef: f64, label: String| {
        out.push(Factor { key: key.into(), group, value, points: value * coef, label });
    };

    if let Some(due) = t.due {
        let (overdue, day) = match due {
            Due::At(at) => (days(now - at), clock.day_of(at)),
            Due::Day(d) => ((today - d).get_days() as f64, d),
        };
        let late = (today - day).get_days();
        let label = if late > 0 { format!("overdue {late}d") } else { format!("due {}", day_label(day, clock)) };
        add("due", Group::Pressure, due_curve(overdue), c.due, label);
    }
    if let Some(s) = t.sched {
        add("sched", Group::Pressure, sched_curve(s, today), c.scheduled, format!("sched {}", day_label(s, clock)));
    }
    if let Some((start, end)) = meeting {
        let mins = (start - now).as_secs_f64() / 60.0;
        let label = if start <= now && now < end { "meeting now".to_string() } else { format!("meeting in {}", dur_label(mins.ceil() as i64)) };
        add("meeting", Group::Pressure, meeting_curve(mins), c.meeting, label);
    }
    if let (Some(at), Some(a)) = (t.last_signal, &t.last_author) {
        if !who.who(&a.source_kind, a.author_id.as_deref(), &a.name).me {
            add("waiting", Group::Pressure, waiting_curve(days(now - at) * 24.0), c.waiting, format!("waiting {}", ago_label(now - at)));
        }
    }
    if t.blocks_open > 0 {
        add("blocks", Group::Value, 1.0, c.blocks, format!("blocks {}", plural(t.blocks_open, "issue", "issues")));
    }
    if let Some(level) = t.topic_priority.as_deref() {
        let (coef, name) = match level {
            "H" => (c.topic_priority.H, "high"),
            "M" => (c.topic_priority.M, "medium"),
            _ => (c.topic_priority.L, "low"),
        };
        add("topic_priority", Group::Value, 1.0, coef, format!("topic: {name}"));
    }
    if let Some(a) = &t.asker {
        let w = who.who(&a.source_kind, a.author_id.as_deref(), &a.name);
        if let Some(p) = w.person_id.and_then(|id| who.person(id)) {
            if !w.me {
                add("who_asked", Group::Value, p.weight.unwrap_or(0.0), c.who_asked, format!("{} asked", p.name));
            }
        }
    }
    if t.topic.is_some() {
        let b = breadth_curve(t.topic_people, t.topic_sources);
        let label = if t.topic_people >= t.topic_sources * 2 { plural(t.topic_people, "person", "people") } else { plural(t.topic_sources, "source", "sources") };
        add("breadth", Group::Value, b, c.breadth, label);
    }
    if let Some((m, _)) = t.estimate {
        add("estimate", Group::Effort, estimate_curve(m), c.estimate, dur_label(m));
    }
    for tag in &t.tags {
        let Some(def) = p.tags.iter().find(|d| d.name.eq_ignore_ascii_case(tag)) else { continue };
        let group = match def.feeds.as_str() {
            "pressure" => Group::Pressure,
            "effort" => Group::Effort,
            _ => Group::Value,
        };
        add(&format!("tag:{}", def.name), group, 1.0, def.coefficient, def.name.clone());
    }
    out
}

/// A reason's colour, by its share of the score (user, 2026-10-06): red for a
/// third or more, yellow from a sixth, green below.
fn share_weight(points: f64, score: f64) -> &'static str {
    let share = if score > 0.0 { points / score } else { 0.0 };
    if share >= 1.0 / 3.0 {
        "red"
    } else if share >= 1.0 / 6.0 {
        "yellow"
    } else {
        "green"
    }
}

// ---------------------------------------------------------------- the list

#[derive(Debug, Serialize, Default)]
pub struct Reasons {
    pub value: Vec<Pill>,
    pub pressure: Vec<Pill>,
    pub effort: Vec<Pill>,
}

#[derive(Debug, Serialize)]
pub struct DateMark {
    /// due (hard) | sched (soft)
    pub kind: &'static str,
    pub label: String,
    /// A hard date that has passed. A soft date is never overdue.
    pub overdue: bool,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub item_id: i64,
    /// local | jira | obsidian
    pub source: String,
    pub title: String,
    pub url: Option<String>,
    pub topic: Option<TopicRef>,
    pub topic_priority: Option<String>,
    /// The hard due date if there is one, else the soft one.
    pub date: Option<DateMark>,
    /// For the Date filter: od | today | week (the next 7 days) | later | none.
    pub when: &'static str,
    pub estimate: Option<super::todos::Estimate>,
    pub score: f64,
    pub value: f64,
    pub pressure: f64,
    pub effort: f64,
    /// For the Score filter: the top, middle or bottom third of the Priority column (hi | mid | lo).
    pub band: &'static str,
    pub neglect: f64,
    pub age_days: i64,
    pub quiet_days: i64,
    /// Factors the todo has (its date, asker, estimate… even when they add 0), for the Factor filter.
    pub factors: Vec<String>,
    /// Factors that added to the score, by group.
    pub reasons: Reasons,
}

#[derive(Debug, Serialize)]
pub struct Orders {
    pub priority: Vec<i64>,
    pub quick: Vec<i64>,
    pub neglect: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct List {
    pub todos: Vec<Row>,
    pub orders: Orders,
    pub quick_max: i64,
    /// The tag vocabulary, for the Factor filter.
    pub tags: Vec<String>,
}

fn source_of(ext: &TodoExt) -> &'static str {
    match ext {
        TodoExt::Local(_) => "local",
        TodoExt::Jira(_) => "jira",
        TodoExt::Obsidian(_) => "obsidian",
    }
}

/// Open todos scored, and the three orders. Jira epics are left out (you track
/// them; the work is in their issues), as are projects the Jira source excludes.
pub fn list(r: &dyn Reads, clock: &Clock, settings: &Settings) -> Result<List> {
    let p = &settings.priorities;
    let now = clock.now();
    let today = clock.today();
    let who = Resolver::load(r, &settings.people)?;
    let skip = super::todos::excluded_projects(r)?;
    let meetings: HashMap<i64, (Ts, Ts)> =
        r.priorities().next_meetings(now, now + SignedDuration::from_hours(8))?.into_iter().map(|(t, s, e)| (t, (s, e))).collect();

    struct Scored {
        row: Row,
        quick_rate: Option<f64>,
        minutes: Option<i64>,
    }
    let mut scored: Vec<Scored> = vec![];
    for t in r.priorities().facts()? {
        let Some(ext) = TodoExt::parse(&t.source_kind, serde_json::from_str::<Value>(&t.raw_json).unwrap_or_default()) else { continue };
        if let TodoExt::Jira(j) = &ext {
            if j.is_epic() || super::todos::project_excluded(&j.key, &skip) {
                continue;
            }
        }
        let meeting = t.topic.as_ref().and_then(|tp| meetings.get(&tp.id)).copied();
        let fs = factors(&t, p, clock, &who, meeting);
        let sum = |g: Group| fs.iter().filter(|f| f.group == g).map(|f| f.points).sum::<f64>();
        let (value, pressure, effort) = (sum(Group::Value), sum(Group::Pressure), sum(Group::Effort));
        let score = value + pressure + effort;

        let mut reasons = Reasons::default();
        for f in fs.iter().filter(|f| f.points > 0.0 && f.key != "estimate") {
            let pill = Pill { label: f.label.clone(), weight: share_weight(f.points, score).into() };
            match f.group {
                Group::Value => reasons.value.push(pill),
                Group::Pressure => reasons.pressure.push(pill),
                Group::Effort => reasons.effort.push(pill),
            }
        }
        // Effort leads with where the estimate came from, coloured by size.
        if let Some((m, by)) = &t.estimate {
            let w = if *m <= 15 { "green" } else if *m <= 60 { "yellow" } else { "red" };
            reasons.effort.insert(0, Pill { label: if by == "you" { "your estimate" } else { "AI guess" }.into(), weight: w.into() });
        }

        let (date, day) = match (t.due, t.sched) {
            (Some(d), _) => {
                let day = match d {
                    Due::At(at) => clock.day_of(at),
                    Due::Day(d) => d,
                };
                (Some(DateMark { kind: "due", label: format!("due {}", day_label(day, clock)), overdue: d.deadline(clock) < now }), Some(day))
            }
            (None, Some(s)) => (Some(DateMark { kind: "sched", label: format!("sched {}", day_label(s, clock)), overdue: false }), Some(s)),
            (None, None) => (None, None),
        };
        let when = match (&date, day) {
            (Some(m), _) if m.overdue => "od",
            (Some(_), Some(d)) => match (d - today).get_days() {
                ..=0 => "today",
                1..=7 => "week",
                _ => "later",
            },
            _ => "none",
        };

        let quiet_from = [Some(t.created), Some(t.changed), t.last_signal].into_iter().flatten().max().unwrap_or(t.created);
        let age = days(now - t.created).max(0.0);
        let quiet = days(now - quiet_from).max(0.0);

        let minutes = t.estimate.as_ref().map(|e| e.0);
        let quick_rate = minutes.filter(|m| *m <= p.quick_max_minutes).map(|m| value / (m.max(1) as f64 / 60.0));
        scored.push(Scored {
            quick_rate,
            minutes,
            row: Row {
                item_id: t.item_id,
                source: source_of(&ext).into(),
                title: t.title,
                url: t.url,
                topic: t.topic,
                topic_priority: t.topic_priority,
                date,
                when,
                estimate: t.estimate.map(|(minutes, by)| super::todos::Estimate { minutes, by }),
                score,
                value,
                pressure,
                effort,
                band: "lo",
                neglect: p.neglect.age * age + p.neglect.quiet * quiet,
                age_days: age.floor() as i64,
                quiet_days: quiet.floor() as i64,
                // A factor is listed only when its fact exists (a date, an asker, an estimate), so
                // it's present even at 0 (a soft date still ahead); breadth needs someone on the topic.
                factors: fs.iter().filter(|f| f.key != "breadth" || f.value > 0.0).map(|f| f.key.clone()).collect(),
                reasons,
            },
        });
    }

    let by = |a: f64, b: f64| b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal);
    // Priority: score, then shorter first.
    let mut priority: Vec<&Scored> = scored.iter().collect();
    priority.sort_by(|a, b| by(a.row.score, b.row.score).then(a.minutes.unwrap_or(i64::MAX).cmp(&b.minutes.unwrap_or(i64::MAX))).then(a.row.item_id.cmp(&b.row.item_id)));
    // Quick wins: under the cut-off by value per hour (deadlines ignored), then the rest by length, then the unestimated by score.
    let mut quick: Vec<&Scored> = scored.iter().collect();
    quick.sort_by(|a, b| {
        let tier = |s: &Scored| if s.quick_rate.is_some() { 0 } else if s.minutes.is_some() { 1 } else { 2 };
        tier(a).cmp(&tier(b)).then_with(|| match tier(a) {
            0 => by(a.quick_rate.unwrap_or(0.0), b.quick_rate.unwrap_or(0.0)).then(a.minutes.cmp(&b.minutes)),
            1 => a.minutes.cmp(&b.minutes).then(by(a.row.score, b.row.score)),
            _ => by(a.row.score, b.row.score),
        })
        .then(a.row.item_id.cmp(&b.row.item_id))
    });
    // Neglect: most neglected first, then oldest.
    let mut neglect: Vec<&Scored> = scored.iter().collect();
    neglect.sort_by(|a, b| by(a.row.neglect, b.row.neglect).then(b.row.age_days.cmp(&a.row.age_days)).then(a.row.item_id.cmp(&b.row.item_id)));

    let n = priority.len();
    let bands: HashMap<i64, &'static str> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.row.item_id, if i * 3 < n { "hi" } else if i * 3 < 2 * n { "mid" } else { "lo" }))
        .collect();
    let orders = Orders {
        priority: priority.iter().map(|s| s.row.item_id).collect(),
        quick: quick.iter().map(|s| s.row.item_id).collect(),
        neglect: neglect.iter().map(|s| s.row.item_id).collect(),
    };
    let todos = scored
        .into_iter()
        .map(|mut s| {
            s.row.band = bands.get(&s.row.item_id).copied().unwrap_or("lo");
            s.row
        })
        .collect();
    Ok(List { todos, orders, quick_max: p.quick_max_minutes, tags: p.tags.iter().map(|t| t.name.clone()).collect() })
}

/// The Priority score of every open item someone owns on a topic, yours and
/// other people's alike (a topic's Open panel mixes them, user 2026-10-07),
/// by the same factors and coefficients as the Priority column.
pub fn topic_scores(r: &dyn Reads, clock: &Clock, settings: &Settings, topic_id: i64) -> Result<HashMap<i64, f64>> {
    let now = clock.now();
    let who = Resolver::load(r, &settings.people)?;
    let meeting = r.priorities().next_meetings(now, now + SignedDuration::from_hours(8))?.into_iter().find(|m| m.0 == topic_id).map(|(_, s, e)| (s, e));
    Ok(r
        .priorities()
        .topic_facts(topic_id)?
        .iter()
        .map(|t| (t.item_id, factors(t, &settings.priorities, clock, &who, meeting).iter().map(|f| f.points).sum()))
        .collect())
}

/// Each open todo's place in the Priority column, from 1.
pub fn ranks(r: &dyn Reads, clock: &Clock, settings: &Settings) -> Result<HashMap<i64, usize>> {
    Ok(list(r, clock, settings)?.orders.priority.into_iter().enumerate().map(|(i, id)| (id, i + 1)).collect())
}

// ---------------------------------------------------------------------------
// Sample data: the todos of `project/Priorities Three Orders.dc.html` (made up).

/// `flags`: b blocks others (Jira links), a someone asked (the signal it came
/// from, or the reporter), w waiting on you (that person spoke last).
struct T {
    src: &'static str,
    title: &'static str,
    topic: &'static str,
    est: i64,
    mine: bool,
    /// Hard (`due`) or soft (`sched`) date, days from today.
    date: Option<(&'static str, i64)>,
    flags: &'static str,
    asker: Option<&'static str>,
    blocks: i64,
    age: i64,
    quiet: i64,
}

pub fn seed(ctx: &crate::seed::Ctx) -> anyhow::Result<()> {
    use crate::domain::sample::TodoExtras;
    use crate::seed::ItemSeed;

    for (topic, level) in [
        ("Q4 headcount", "H"),
        ("Acme outage", "H"),
        ("Board prep", "H"),
        ("Sr. EM offer", "M"),
        ("Datadog renewal", "M"),
        ("Webhook reliability", "M"),
        ("Staff loop", "L"),
        ("Team growth", "L"),
        ("Conference CFP", "L"),
    ] {
        ctx.sample().topic_priority(ctx.topic(topic), level)?;
    }

    let t = |src, title, topic, est, mine, date, flags, asker, blocks, age, quiet| T { src, title, topic, est, mine, date, flags, asker, blocks, age, quiet };
    let rows = [
        t("local", "Send Platform vs Data split to Priya", "Q4 headcount", 30, true, Some(("due", 0)), "a", Some("Priya"), 0, 2, 0),
        t("local", "Reply to Dana with the outage timeline", "Acme outage", 20, false, Some(("due", 0)), "aw", Some("Dana"), 0, 1, 0),
        t("local", "Approve Sr. EM offer", "Sr. EM offer", 10, false, Some(("due", 1)), "a", Some("Lee"), 0, 5, 1),
        t("jira", "PLAT-1204 · review webhook retry PR", "Webhook reliability", 15, false, None, "aw", Some("Marco"), 0, 3, 2),
        t("jira", "PROC-412 · sign off the Datadog PO", "Datadog renewal", 5, false, Some(("due", 2)), "ba", Some("Kim"), 1, 6, 4),
        t("obsidian", "Q4 strategy doc, first draft", "Q4 planning", 240, true, None, "", None, 0, 21, 9),
        t("obsidian", "Director narrative, one page", "Career", 120, true, None, "", None, 0, 64, 64),
        t("local", "Hand off disk-pressure tuning to Jordan", "Alerting · P2", 10, true, None, "", None, 0, 9, 0),
        t("jira", "PLAT-1173 · retry limits ADR", "Webhook reliability", 90, true, None, "b", None, 3, 18, 11),
        t("local", "Datadog counter-offer numbers", "Datadog renewal", 45, false, Some(("due", 1)), "", None, 0, 8, 1),
        t("local", "Book the Q4 offsite room", "Q4 planning", 10, false, Some(("due", 3)), "", None, 0, 12, 12),
        t("jira", "PLAT-1219 · renew staging webhook secret", "Webhook reliability", 15, false, None, "", None, 0, 15, 15),
        t("local", "Thank Lee for covering the pager", "Acme outage", 5, false, None, "", None, 0, 4, 4),
        t("obsidian", "Interview rubric for the EM loop", "Sr. EM offer", 60, false, None, "", None, 0, 10, 6),
        t("local", "Ask Kim for the Acme RCA draft", "Acme outage", 5, false, Some(("due", 2)), "", None, 0, 3, 0),
        t("jira", "DEVOPS-88 · retire the v1 ingest cron", "v1 ingest deprecation", 30, false, None, "b", None, 1, 47, 30),
        t("obsidian", "Alert-noise baseline write-up", "Alerting · P2", 120, true, None, "", None, 0, 35, 22),
        t("local", "Reply to the CFP co-author", "Conference CFP", 10, false, Some(("sched", 2)), "aw", Some("Sam"), 0, 6, 1),
        t("obsidian", "Talk abstract outline", "Conference CFP", 60, false, Some(("due", 40)), "", None, 0, 40, 40),
        t("jira", "PLAT-1240 · retry queue depth dashboard", "Webhook reliability", 180, false, None, "", None, 0, 88, 53),
        t("local", "Tell the team the headcount outcome", "Q4 headcount", 15, false, Some(("sched", 2)), "", None, 0, 1, 1),
        t("local", "Expense the Datadog workshop", "Datadog renewal", 10, false, Some(("due", -1)), "", None, 0, 14, 14),
        t("obsidian", "Skip-level notes for Priya", "Staff loop", 20, false, Some(("sched", 1)), "", None, 0, 7, 7),
        t("jira", "DEVOPS-91 · ingest cutover runbook", "v1 ingest deprecation", 120, false, None, "b", None, 1, 52, 41),
        t("local", "Approve PTO requests", "Staff loop", 5, false, Some(("due", 1)), "aw", Some("Lee"), 0, 3, 3),
        t("obsidian", "Mentoring plan for Sam", "Team growth", 45, false, None, "", None, 0, 71, 71),
        t("jira", "PLAT-1251 · fix the flaky webhook test", "Webhook reliability", 30, false, None, "", None, 0, 120, 34),
        t("local", "Board-prep numbers to Finance", "Board prep", 20, false, Some(("due", 2)), "a", Some("Ana"), 0, 2, 0),
        t("obsidian", "Reading group, SRE ch. 6", "Team growth", 45, false, Some(("sched", 3)), "", None, 0, 30, 30),
        t("local", "Reply to the recruiter about the panel", "Sr. EM offer", 5, false, None, "aw", Some("Lee"), 0, 4, 1),
        t("jira", "PLAT-1199 · decide retry caps", "Webhook reliability", 20, false, None, "b", None, 2, 26, 19),
        t("obsidian", "Delegation map for Q1", "Q4 planning", 90, true, None, "", None, 0, 58, 58),
        t("local", "Check alert routing after the Opsgenie change", "Alerting · P2", 15, false, None, "", None, 0, 11, 11),
        t("local", "Order team dinner for the offsite", "Q4 planning", 10, false, Some(("sched", 3)), "", None, 0, 9, 9),
    ];

    for (n, r) in rows.iter().enumerate() {
        let created = ctx.at(-r.age, "09:00");
        let changed = ctx.at(-r.quiet, if r.quiet == 0 { "09:30" } else { "09:00" });
        let due = match r.date {
            Some(("due", d)) => Some(crate::time::Due::Day(ctx.today + jiff::ToSpan::days(d))),
            _ => None,
        };
        let sched_day = match r.date {
            Some(("sched", d)) => Some(ctx.today + jiff::ToSpan::days(d)),
            _ => None,
        };
        let (source, kind, author) = match r.src {
            "jira" => ("jira", "issue", r.asker),
            "obsidian" => ("obsidian", "task", None),
            _ => ("notes", "task", None),
        };
        let key = r.title.split(" · ").next().unwrap_or_default();
        let url = match r.src {
            "jira" => format!("https://jira.example.com/browse/{key}"),
            "obsidian" => format!("obsidian://open?vault=work&file={}", r.topic.replace(' ', "%20")),
            _ => String::new(),
        };
        let id = ctx.item(ItemSeed {
            source,
            kind,
            title: r.title.into(),
            url: (!url.is_empty()).then_some(url),
            author,
            occurred_at: if r.src == "jira" { changed } else { created },
            due,
            owner: Some("you".into()),
            ..Default::default()
        });
        ctx.link(id, r.topic, "confirmed");

        // The signal a local todo came from, with the asker's message.
        let mut from_item = None;
        if r.src == "local" {
            if let Some(who) = r.asker {
                let signal = ctx.item(ItemSeed {
                    source: "gmail",
                    kind: "email",
                    title: format!("{who}: {}", r.title),
                    author: Some(who),
                    occurred_at: created,
                    seen: true,
                    ..Default::default()
                });
                if r.flags.contains('w') {
                    ctx.message(signal, changed, who, "Any update on this?");
                }
                from_item = Some((signal, format!("{who}: {}", r.title)));
            }
        }
        if r.src == "jira" && r.flags.contains('w') {
            if let Some(who) = r.asker {
                ctx.message(id, changed, who, "Could you take a look when you get a minute?");
            }
        }

        let raw = match r.src {
            "jira" => {
                let links: Vec<Value> = (0..r.blocks)
                    .map(|i| {
                        json!({ "type": { "name": "Blocks", "inward": "is blocked by", "outward": "blocks" },
                                "outwardIssue": { "key": format!("{}-{}", key.split('-').next().unwrap_or("PLAT"), 2000 + n * 10 + i as usize),
                                                  "fields": { "status": { "name": "To Do", "statusCategory": { "key": "new" } } } } })
                    })
                    .collect();
                json!({ "key": key, "fields": { "summary": r.title.split(" · ").nth(1).unwrap_or(r.title), "status": { "name": "In Progress" },
                                               "issuetype": { "name": "Task" }, "issuelinks": links } })
            }
            "obsidian" => json!({ "path": format!("Topics/{}.md", r.topic), "text": r.title, "line": 3 + n }),
            _ => json!({ "from": from_item.as_ref().map(|f| f.1.clone()), "from_item": from_item.as_ref().map(|f| f.0) }),
        };
        let raw = raw.to_string();
        ctx.sample().todo_extras(
            id,
            &TodoExtras {
                raw_json: &raw,
                sched_day,
                source_created_at: (r.src == "jira").then_some(created),
                estimate: Some((r.est, if r.mine { "you" } else { "ai" })),
                created_at: created,
                updated_at: changed,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::App;
    use rusqlite::Connection;

    const NOW: &str = "2026-09-24T10:52:00Z";

    fn setup() -> (Connection, Clock, tempfile::TempDir) {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let clock = crate::sync::test_clock(NOW);
        crate::seed::seed(&crate::store::over(&conn), &clock, dir.path()).unwrap();
        (conn, clock, dir)
    }

    fn scored(conn: &Connection, clock: &Clock, settings: &Settings) -> List {
        list(&crate::store::over(conn), clock, settings).unwrap()
    }

    fn row<'a>(l: &'a List, title: &str) -> &'a Row {
        l.todos.iter().find(|r| r.title == title).unwrap_or_else(|| panic!("no todo {title}"))
    }

    fn id_of(conn: &Connection, title: &str) -> i64 {
        conn.query_row("SELECT id FROM items WHERE title = ?1", [title], |r| r.get(0)).unwrap()
    }

    #[test]
    fn curves_match_taskwarrior_where_it_has_the_factor() {
        assert_eq!(due_curve(-30.0), 0.2);
        assert_eq!(due_curve(-14.0), 0.2);
        assert!((due_curve(0.0) - (0.2 + 14.0 * 0.8 / 21.0)).abs() < 1e-9);
        assert_eq!(due_curve(7.0), 1.0);
        let d = |s: &str| s.parse::<Date>().unwrap();
        assert_eq!(sched_curve(d("2026-09-25"), d("2026-09-24")), 0.0);
        assert_eq!(sched_curve(d("2026-09-24"), d("2026-09-24")), 1.0);
        assert_eq!(sched_curve(d("2026-09-01"), d("2026-09-24")), 1.0);
        assert_eq!(estimate_curve(5), 1.0);
        assert_eq!(estimate_curve(240), 0.0);
        assert!(estimate_curve(30) > estimate_curve(60));
        assert_eq!(meeting_curve(10.0), 1.0);
        assert_eq!(meeting_curve(480.0), 0.0);
        assert_eq!(waiting_curve(72.0), 1.0);
        assert_eq!(breadth_curve(10, 5), 1.0);
        assert_eq!(breadth_curve(0, 0), 0.0);
    }

    #[test]
    fn every_column_holds_every_todo_and_epics_are_left_out() {
        let (conn, clock, _d) = setup();
        let epic = id_of(&conn, "PLAT-1240 · retry queue depth dashboard");
        conn.execute("UPDATE items SET raw_json = json_set(raw_json, '$.fields.issuetype.name', 'Epic') WHERE id = ?1", [epic]).unwrap();
        let l = scored(&conn, &clock, &Settings::default());
        assert_eq!(l.todos.len(), 33, "the mock's 34, less the epic");
        assert!(!l.todos.iter().any(|r| r.item_id == epic));
        let mut sets = [l.orders.priority.clone(), l.orders.quick.clone(), l.orders.neglect.clone()];
        for s in &mut sets {
            s.sort();
        }
        assert_eq!(sets[0], sets[1]);
        assert_eq!(sets[1], sets[2]);
        assert_eq!(sets[0].len(), 33);
    }

    #[test]
    fn scores_are_the_sum_of_factor_times_coefficient() {
        let (conn, clock, _d) = setup();
        let l = scored(&conn, &clock, &Settings::default());
        // Due today (Taskwarrior: 0.733 × 12) under a high-priority topic (6.0).
        let send = row(&l, "Send Platform vs Data split to Priya");
        let due_today = (0.2 + 14.0 * 0.8 / 21.0) * 12.0;
        assert!((send.pressure - due_today).abs() < 1e-9, "{}", send.pressure);
        assert_eq!(send.value, 6.0);
        assert!((send.score - (due_today + 6.0)).abs() < 1e-9);
        assert_eq!(send.reasons.pressure[0].label, "due today");
        assert_eq!(send.reasons.pressure[0].weight, "red", "more than a third of the score");
        assert_eq!(send.reasons.value[0].label, "topic: high");
        assert_eq!(send.date.as_ref().unwrap().kind, "due");
        assert_eq!(send.when, "today");
        // Factors at coefficient 0 count as present but add nothing and show no pill.
        assert!(send.factors.contains(&"meeting".to_string()), "the 11:00 Q4 headcount");
        assert!(!send.reasons.pressure.iter().any(|p| p.label.starts_with("meeting")));
        // Effort leads with where its estimate came from.
        assert_eq!(send.reasons.effort[0].label, "your estimate");

        let adr = row(&l, "PLAT-1173 · retry limits ADR");
        assert_eq!(adr.value, 8.0 + 3.9, "blocks open issues, medium topic");
        assert_eq!(adr.reasons.value[0].label, "blocks 3 issues");

        // A scheduled day still ahead adds nothing; Taskwarrior's step starts on the day.
        let skip = row(&l, "Skip-level notes for Priya");
        assert_eq!(skip.pressure, 0.0);
        assert_eq!(skip.date.as_ref().unwrap().kind, "sched");

        let late = row(&l, "Expense the Datadog workshop");
        assert!(late.date.as_ref().unwrap().overdue);
        assert_eq!(late.when, "od");
        assert_eq!(late.reasons.pressure[0].label, "overdue 1d");

        // Priority is by score.
        let first = l.orders.priority.iter().map(|id| l.todos.iter().find(|r| r.item_id == *id).unwrap().score).collect::<Vec<_>>();
        assert!(first.windows(2).all(|w| w[0] >= w[1]));
        let top = l.todos.iter().find(|r| r.item_id == l.orders.priority[0]).unwrap();
        assert_eq!(top.band, "hi");
    }

    #[test]
    fn a_soft_date_still_ahead_is_present_for_the_factor_filter() {
        let (conn, clock, _d) = setup();
        let l = scored(&conn, &clock, &Settings::default());
        let skip = row(&l, "Skip-level notes for Priya");
        assert_eq!(skip.pressure, 0.0);
        assert!(skip.factors.contains(&"sched".to_string()));
    }

    #[test]
    fn a_coefficient_from_settings_turns_a_factor_on() {
        let (conn, clock, _d) = setup();
        let dana = |s: &Settings| {
            let l = scored(&conn, &clock, s);
            let r = row(&l, "Reply to Dana with the outage timeline");
            (r.pressure, r.reasons.pressure.iter().map(|p| p.label.clone()).collect::<Vec<_>>())
        };
        let (before, pills) = dana(&Settings::default());
        assert!(!pills.iter().any(|p| p.starts_with("waiting")));
        let mut s = Settings::default();
        s.priorities.coefficients.waiting = 4.0;
        let (after, pills) = dana(&s);
        assert!(after > before);
        assert!(pills.iter().any(|p| p.starts_with("waiting")), "{pills:?}");
    }

    #[test]
    fn quick_wins_rank_by_value_per_hour_under_the_cut_off_then_by_length() {
        let (conn, clock, _d) = setup();
        let l = scored(&conn, &clock, &Settings::default());
        let rows: Vec<&Row> = l.orders.quick.iter().map(|id| l.todos.iter().find(|r| r.item_id == *id).unwrap()).collect();
        let mins = |r: &Row| r.estimate.as_ref().unwrap().minutes;
        let cut = rows.iter().position(|r| mins(r) > 30).unwrap();
        assert!(rows[..cut].iter().all(|r| mins(r) <= 30));
        let rate = |r: &Row| r.value / (mins(r) as f64 / 60.0);
        assert!(rows[..cut].windows(2).all(|w| rate(w[0]) >= rate(w[1])));
        assert!(rows[cut..].windows(2).all(|w| mins(w[0]) <= mins(w[1])), "past the cut-off, shortest first");
        // A todo with no estimate goes last.
        let thank = id_of(&conn, "Thank Lee for covering the pager");
        conn.execute("DELETE FROM todo_estimates WHERE item_id = ?1", [thank]).unwrap();
        let l = scored(&conn, &clock, &Settings::default());
        assert_eq!(*l.orders.quick.last().unwrap(), thank);
    }

    #[test]
    fn neglect_weighs_age_and_quiet_time() {
        let (conn, clock, _d) = setup();
        let l = scored(&conn, &clock, &Settings::default());
        let sam = row(&l, "Mentoring plan for Sam");
        assert_eq!((sam.age_days, sam.quiet_days), (71, 71));
        let jira = row(&l, "PLAT-1251 · fix the flaky webhook test");
        assert_eq!((jira.age_days, jira.quiet_days), (120, 34), "a Jira issue's age is its own created time");
        let ns: Vec<f64> = l.orders.neglect.iter().map(|id| l.todos.iter().find(|r| r.item_id == *id).unwrap().neglect).collect();
        assert!(ns.windows(2).all(|w| w[0] >= w[1]));
        assert!((jira.neglect - (0.4 * 120.0 + 0.6 * 34.0)).abs() < 0.1, "{}", jira.neglect);

        // Any change to the todo ends its quiet time; so does a topic change.
        let id = id_of(&conn, "Mentoring plan for Sam");
        conn.execute("UPDATE items SET title = 'Mentoring plan for Sam, v2' WHERE id = ?1", [id]).unwrap();
        let l = list(&crate::store::over(&conn), &crate::sync::test_clock(&crate::time::Ts::parse(NOW).unwrap().to_db()), &Settings::default()).unwrap();
        // The trigger stamps real time, which is after the test clock: quiet time can't go below 0.
        assert_eq!(row(&l, "Mentoring plan for Sam, v2").quiet_days, 0);
    }

    #[test]
    fn a_bots_message_isnt_the_last_signal() {
        let (conn, _clock, _d) = setup();
        // A Jira todo's thread is the issue itself; Marco asked on it.
        let jira = id_of(&conn, "PLAT-1204 · review webhook retry PR");
        let last = |conn: &Connection| {
            let f = crate::store::over(conn).priorities().facts().unwrap().into_iter().find(|f| f.item_id == jira).unwrap();
            (f.last_signal.map(|t| t.to_db()), f.last_author.map(|a| a.name))
        };
        let before = last(&conn);
        assert_eq!(before.1.as_deref(), Some("Marco"));
        conn.execute("INSERT INTO account_profiles (kind, account, real_name, email, is_bot) VALUES ('jira', 'bot-1', 'Automation', '', 1)", []).unwrap();
        conn.execute(
            "INSERT INTO item_messages (item_id, at, author, text, ext_key, author_id) VALUES (?1, '2026-09-24T10:00:00.000Z', 'Automation', 'Moved', 'jira:b1', 'bot-1')",
            [jira],
        )
        .unwrap();
        assert_eq!(last(&conn), before, "the bot's comment is skipped");
        conn.execute(
            "INSERT INTO item_messages (item_id, at, author, text, ext_key, author_id) VALUES (?1, '2026-09-24T10:30:00.000Z', 'Kim', 'Me too', 'jira:k1', 'kim-1')",
            [jira],
        )
        .unwrap();
        assert_eq!(last(&conn), (Some("2026-09-24T10:30:00.000Z".into()), Some("Kim".into())));
    }

    #[test]
    fn only_open_issues_this_one_blocks_count() {
        let (conn, _clock, _d) = setup();
        let id = id_of(&conn, "PLAT-1251 · fix the flaky webhook test");
        let link = |dir: &str, key: &str, cat: &str| {
            json!({ "type": { "name": "Blocks", "inward": "is blocked by", "outward": "blocks" },
                    dir: { "key": key, "fields": { "status": { "name": "x", "statusCategory": { "key": cat } } } } })
        };
        let links = json!([link("outwardIssue", "PLAT-1", "new"), link("outwardIssue", "PLAT-2", "done"), link("inwardIssue", "PLAT-3", "new"),
                           { "type": { "name": "Relates", "inward": "relates to", "outward": "relates to" }, "outwardIssue": { "key": "PLAT-4" } }]);
        conn.execute("UPDATE items SET raw_json = json_set(raw_json, '$.fields.issuelinks', json(?2)) WHERE id = ?1", rusqlite::params![id, links.to_string()]).unwrap();
        let f = crate::store::over(&conn).priorities().facts().unwrap().into_iter().find(|f| f.item_id == id).unwrap();
        assert_eq!(f.blocks_open, 1, "an open outward Blocks link; not done, inward or another link type");
    }

    #[test]
    fn delegated_asks_are_scored_the_same_and_are_never_quick_wins() {
        let (conn, clock, dir) = setup();
        let notes: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'notes'", [], |r| r.get(0)).unwrap();
        for (ext, title, owner) in [("notes-line-901", "@Jordan check the retry storm's reach", Some("Jordan")), ("notes-line-902", "^ someone books the room", None)] {
            conn.execute(
                "INSERT INTO items (source_id, external_id, kind, title, occurred_at, status, owner, seen, raw_json)
                 VALUES (?1, ?2, 'task', ?3, '2026-09-24T09:00:00.000Z', 'open', ?4, 1, '{\"notes_line_id\": 901, \"kind\": \"delegated\"}')",
                rusqlite::params![notes, ext, title, owner],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, (SELECT id FROM topics WHERE name = 'Acme outage'), 'confirmed')", [id]).unwrap();
        }
        let l = scored(&conn, &clock, &Settings::default());
        let ask = row(&l, "@Jordan check the retry storm's reach");
        assert_eq!(ask.value, 6.0, "its topic's priority, as for your own todos");
        assert!(ask.estimate.is_none());
        let unnamed = row(&l, "^ someone books the room").item_id;
        let quick = &l.orders.quick;
        let first_unestimated = quick.iter().position(|id| l.todos.iter().find(|r| r.item_id == *id).unwrap().estimate.is_none()).unwrap();
        assert!(quick[first_unestimated..].contains(&ask.item_id) && quick[first_unestimated..].contains(&unnamed), "at the bottom of Quick wins");
        // They aren't your todos: the Todos tab doesn't list them, so the AI doesn't estimate them.
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        let todos = crate::api::call(&app, "todos.list", &json!({})).unwrap();
        assert!(!todos["todos"].as_array().unwrap().iter().any(|t| t["item_id"] == ask.item_id));
    }

    #[test]
    fn personal_and_done_todos_are_left_out() {
        let (conn, clock, _d) = setup();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at, owner) VALUES
               ((SELECT id FROM sources WHERE kind = 'marvin'), 'x1', 'task', 'Personal thing', '2026-09-24T09:00:00.000Z', 'you')",
            [],
        )
        .unwrap();
        conn.execute("UPDATE items SET status = 'done' WHERE title = 'Approve Sr. EM offer'", []).unwrap();
        let l = scored(&conn, &clock, &Settings::default());
        assert!(!l.todos.iter().any(|r| r.title == "Personal thing" || r.title == "Approve Sr. EM offer"));
    }

    #[test]
    fn call_routes_list_and_todos_carry_their_rank() {
        let (conn, _clock, dir) = setup();
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        let v = crate::api::call(&app, "priorities.list", &json!({})).unwrap();
        assert!(v["todos"].as_array().unwrap().len() >= 30);
        let first = v["orders"]["priority"][0].as_i64().unwrap();
        let todos = crate::api::call(&app, "todos.list", &json!({})).unwrap();
        let t = todos["todos"].as_array().unwrap().iter().find(|t| t["item_id"] == first).unwrap();
        assert_eq!(t["rank"], 1);
        assert!(crate::api::call(&app, "priorities.nope", &json!({})).is_err());
    }

    #[test]
    fn a_local_todo_can_be_scheduled_and_others_cant() {
        let (conn, _clock, dir) = setup();
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        let local = id_of(&app.db(), "Book the Q4 offsite room");
        crate::api::call(&app, "todos.schedule", &json!({ "item_id": local, "sched": "2026-09-30" })).unwrap();
        let day: Option<String> = app.db().query_row("SELECT sched_day FROM items WHERE id = ?1", [local], |r| r.get(0)).unwrap();
        assert_eq!(day.as_deref(), Some("2026-09-30"));
        crate::api::call(&app, "todos.schedule", &json!({ "item_id": local, "sched": null })).unwrap();
        let day: Option<String> = app.db().query_row("SELECT sched_day FROM items WHERE id = ?1", [local], |r| r.get(0)).unwrap();
        assert_eq!(day, None);
        let jira = id_of(&app.db(), "PLAT-1251 · fix the flaky webhook test");
        assert!(crate::api::call(&app, "todos.schedule", &json!({ "item_id": jira, "sched": "2026-09-30" })).is_err());
        // Made with one.
        crate::api::call(&app, "todos.create", &json!({ "target": "local", "text": "Plan the offsite agenda", "sched": "2026-09-24" })).unwrap();
        let l = app.store().read(|r| list(r, &app.clock, &Settings::default())).unwrap();
        let r = row(&l, "Plan the offsite agenda");
        assert_eq!(r.date.as_ref().unwrap().kind, "sched");
        assert!(crate::api::call(&app, "todos.create", &json!({ "target": "jira", "text": "x", "sched": "2026-09-24" })).is_err());
    }

    #[test]
    fn a_topic_priority_is_h_m_l_or_none() {
        let (conn, _clock, dir) = setup();
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        let id: i64 = app.db().query_row("SELECT id FROM topics WHERE name = 'On-call load'", [], |r| r.get(0)).unwrap();
        crate::api::call(&app, "topics.set_priority", &json!({ "id": id, "priority": "H" })).unwrap();
        let p: Option<String> = app.db().query_row("SELECT priority FROM topics WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(p.as_deref(), Some("H"));
        crate::api::call(&app, "topics.set_priority", &json!({ "id": id, "priority": null })).unwrap();
        let p: Option<String> = app.db().query_row("SELECT priority FROM topics WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(p, None);
        assert!(crate::api::call(&app, "topics.set_priority", &json!({ "id": id, "priority": "X" })).is_err());
    }
}

//! The People index (design canvas "People and the graph", frames `PeopleSort`
//! and `PeopleTop3Filtered`; decisions in `app/docs/decisions.md` › "People tab
//! design").
//!
//! - A person's signals are the **confirmed** work signals they wrote in (a
//!   message, or the item's author), matched to People notes at read time like
//!   everywhere else (`people::Resolver`). You're left out.
//! - The 30-day total and the per-topic counts cover the last 30 days; the
//!   strip is 12 calendar weeks starting Monday, the last one this week so far.
//!   A signal counts once per week (and once in the total) however many
//!   messages they wrote in it.
//! - Each row lists every topic the person has ever had a confirmed signal in
//!   (the topic filter matches on that), with its 30-day count; the UI shows
//!   the top three and the filter's columns from it.
//! - Groups: `recent` (a signal in 30 days), `quiet` (every other People note,
//!   signals or not), `unknown` (a sender with no note and a signal in 30 days;
//!   not bots, not mail from lists or systems).
//! - Next meeting: the soonest timed event within 7 days on the work calendar
//!   with them as a guest (by email), not declined by you (those have no
//!   `events` row) or by them.

use super::people::{hash_color, is_bulk, Account, Resolver};
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::ports::Reads;
use crate::error::Result;
use crate::settings::PeopleSettings;
use crate::time::Ts;
use jiff::{SignedDuration, ToSpan};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Weeks in the strip.
pub const WEEKS: usize = 12;
/// Days that make a person recent, and that the total and topic counts cover.
pub const RECENT_DAYS: i64 = 30;
/// How far ahead Next meeting looks.
pub const MEETING_DAYS: i64 = 7;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TopicCount {
    pub id: i64,
    pub name: String,
    pub category: String,
    /// Confirmed signals in the last 30 days (0 when only older ones).
    pub n: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Meeting {
    pub start: Ts,
    pub title: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct Row {
    /// "p:<person id>" or "a:<kind>:<account>".
    pub key: String,
    /// The People note's person; None for a sender with no note.
    pub id: Option<i64>,
    pub name: String,
    pub role: Option<String>,
    /// Palette index.
    pub color: u8,
    pub known: bool,
    /// For Link to… on a sender with no note.
    pub account: Option<Account>,
    /// What `/` matches besides the name and role: aliases and accounts, lowercased.
    pub terms: Vec<String>,
    /// `recent` | `quiet` | `unknown`.
    pub group: &'static str,
    /// Confirmed signals in the last 30 days.
    pub total: i64,
    /// Every topic with a confirmed signal of theirs, most in 30 days first.
    pub topics: Vec<TopicCount>,
    /// Confirmed signals per week, oldest first; the last is this week.
    pub weeks: Vec<i64>,
    pub last_at: Option<Ts>,
    /// "today 09:40", "Oct 20".
    pub last: Option<String>,
    /// Source kind of the last signal, for its icon.
    pub last_source: Option<String>,
    pub next_meeting: Option<Meeting>,
}

/// Per person or sender while reading.
struct Acc {
    weeks: Vec<HashSet<i64>>,
    recent: HashSet<i64>,
    by_topic: HashMap<i64, HashSet<i64>>,
    /// Every topic, with the latest time there.
    topics: HashMap<i64, Ts>,
    last: Option<(Ts, String)>,
    /// Latest display name, for a sender with no note.
    name: Option<(Ts, String)>,
    meeting: Option<Meeting>,
}

impl Acc {
    fn new() -> Acc {
        Acc {
            weeks: vec![HashSet::new(); WEEKS],
            recent: HashSet::new(),
            by_topic: HashMap::new(),
            topics: HashMap::new(),
            last: None,
            name: None,
            meeting: None,
        }
    }
}

/// "today 09:40" today, else "Oct 20".
fn last_label(t: Ts, clock: &Clock) -> String {
    let t = clock.local(t);
    if t.date() == clock.today() {
        format!("today {}", t.strftime("%H:%M"))
    } else {
        t.strftime("%b %-d").to_string()
    }
}

/// The first day of the strip: the Monday 11 weeks before this week's.
pub fn strip_start(clock: &Clock) -> jiff::civil::Date {
    let today = clock.today();
    let monday = today - (today.weekday().to_monday_zero_offset() as i64).days();
    monday - ((WEEKS as i64 - 1) * 7).days()
}

/// Which strip week an instant falls in (0 the oldest), if any.
pub fn week_of(clock: &Clock, start: jiff::civil::Date, at: Ts) -> Option<usize> {
    let w = (clock.day_of(at) - start).get_days().div_euclid(7);
    usize::try_from(w).ok().filter(|&w| w < WEEKS)
}

pub fn index(reads: &dyn Reads, settings: &PeopleSettings, clock: &Clock) -> Result<Vec<Row>> {
    let r = Resolver::load(reads, settings)?;
    let now = clock.now();
    let recent_since = now - SignedDuration::from_hours(24 * RECENT_DAYS);
    let start = strip_start(clock);
    let since = clock.day_range(start).0.min(recent_since);
    let bots: HashSet<(String, String)> = reads.people().bots()?.into_iter().collect();

    let mut acc: HashMap<String, Acc> = HashMap::new();
    // The person's key, or None for you and for authors nobody can link.
    let key_of = |kind: &str, handle: Option<&str>, name: &str| -> Option<(String, Option<Account>)> {
        let who = r.who(kind, handle, name);
        if who.me {
            return None;
        }
        if let Some(id) = who.person_id {
            return Some((format!("p:{id}"), None));
        }
        let a = who.account?;
        let pair = (a.kind.clone(), a.id.clone());
        if bots.contains(&pair) || is_bulk(&pair) {
            return None;
        }
        Some((format!("a:{}:{}", a.kind, a.id), Some(a)))
    };
    let mut accounts: HashMap<String, Account> = HashMap::new();

    for a in reads.people().signal_authors_since(since)? {
        let Some((key, account)) = key_of(&a.source_kind, a.handle.as_deref(), &a.name) else {
            continue;
        };
        if let Some(account) = account {
            accounts.insert(key.clone(), account);
        }
        let e = acc.entry(key).or_insert_with(Acc::new);
        if let Some(w) = week_of(clock, start, a.at) {
            e.weeks[w].insert(a.item_id);
        }
        if a.at >= recent_since {
            e.recent.insert(a.item_id);
            e.by_topic.entry(a.topic_id).or_default().insert(a.item_id);
            if e.name.as_ref().is_none_or(|(t, _)| a.at >= *t) {
                e.name = Some((a.at, a.name.trim().to_string()));
            }
        }
    }
    for a in reads.people().author_topics()? {
        let Some((key, account)) = key_of(&a.source_kind, a.handle.as_deref(), &a.name) else {
            continue;
        };
        // Senders with no note are listed only with a signal in 30 days.
        if account.is_some() && !acc.contains_key(&key) {
            continue;
        }
        let e = acc.entry(key).or_insert_with(Acc::new);
        let t = e.topics.entry(a.topic_id).or_insert(a.last_at);
        *t = (*t).max(a.last_at);
        if e.last.as_ref().is_none_or(|(t, _)| a.last_at > *t) {
            e.last = Some((a.last_at, a.source_kind));
        }
    }
    let until = now + SignedDuration::from_hours(24 * MEETING_DAYS);
    for g in reads.people().upcoming_guests(now, until)? {
        if g.response.as_deref() == Some("declined") {
            continue;
        }
        let who = r.who("gcal", Some(&g.email), "");
        let key = match (who.me, who.person_id) {
            (true, _) => continue,
            (false, Some(id)) => format!("p:{id}"),
            (false, None) => format!("a:email:{}", g.email),
        };
        if let Some(e) = acc.get_mut(&key).filter(|e| e.meeting.is_none()) {
            e.meeting = Some(Meeting {
                start: g.start,
                title: g.title,
            });
        } else if key.starts_with("p:") && !acc.contains_key(&key) {
            let mut e = Acc::new();
            e.meeting = Some(Meeting {
                start: g.start,
                title: g.title,
            });
            acc.insert(key, e);
        }
    }

    let dir = reads.people().directory()?;
    let aliases: HashMap<i64, Vec<String>> = dir.people.iter().map(|p| (p.id, p.aliases.clone())).collect();
    let mut person_accounts: HashMap<i64, Vec<String>> = HashMap::new();
    for a in &dir.accounts {
        person_accounts.entry(a.person_id).or_default().push(a.account.clone());
    }
    let mut topic_refs: HashMap<i64, crate::domain::topics::TopicRef> = HashMap::new();
    let mut topic = |id: i64| -> Result<Option<crate::domain::topics::TopicRef>> {
        if let Some(t) = topic_refs.get(&id) {
            return Ok(Some(t.clone()));
        }
        let t = reads.topics().get_ref(id)?;
        if let Some(t) = &t {
            topic_refs.insert(id, t.clone());
        }
        Ok(t)
    };

    let mut rows = vec![];
    // Every People note, with or without signals.
    let mut people: Vec<_> = r.people().collect();
    people.sort_by_key(|p| p.id);
    let mut keys: Vec<(String, Option<i64>)> = people.iter().map(|p| (format!("p:{}", p.id), Some(p.id))).collect();
    let mut unknown: Vec<String> = acc.keys().filter(|k| k.starts_with("a:")).cloned().collect();
    unknown.sort();
    keys.extend(unknown.into_iter().map(|k| (k, None)));
    for (key, id) in keys {
        let e = acc.remove(&key).unwrap_or_else(Acc::new);
        let mut topics = vec![];
        let mut latest: Vec<(i64, Ts)> = e.topics.iter().map(|(k, v)| (*k, *v)).collect();
        latest.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (tid, _) in latest {
            if let Some(t) = topic(tid)? {
                topics.push(TopicCount {
                    id: t.id,
                    name: t.name,
                    category: t.category,
                    n: e.by_topic.get(&tid).map_or(0, |s| s.len() as i64),
                });
            }
        }
        // Most in 30 days first; ties keep the most recent first (the order above).
        topics.sort_by(|a, b| b.n.cmp(&a.n));
        let total = e.recent.len() as i64;
        let weeks = e.weeks.iter().map(|w| w.len() as i64).collect();
        let (last_at, last_source) = e.last.map_or((None, None), |(t, s)| (Some(t), Some(s)));
        let last = last_at.map(|t| last_label(t, clock));
        match id.and_then(|id| r.person(id)) {
            Some(p) => {
                let mut terms: Vec<String> = aliases.get(&p.id).into_iter().flatten().cloned().collect();
                terms.extend(person_accounts.get(&p.id).into_iter().flatten().cloned());
                rows.push(Row {
                    key,
                    id: Some(p.id),
                    name: p.name.clone(),
                    role: p.role.clone(),
                    color: p.color,
                    known: true,
                    account: None,
                    terms: terms.into_iter().map(|t| t.to_lowercase()).collect(),
                    group: if total > 0 { "recent" } else { "quiet" },
                    total,
                    topics,
                    weeks,
                    last_at,
                    last,
                    last_source,
                    next_meeting: e.meeting,
                });
            }
            None => {
                let Some(account) = accounts.remove(&key) else {
                    continue;
                };
                if total == 0 {
                    continue;
                }
                let email = if account.kind == "email" {
                    Some(account.id.clone())
                } else {
                    r.profile_email(&account.kind, &account.id).map(str::to_owned)
                };
                // The address itself for mail; the profile's real name, else the latest name seen.
                let name = if account.kind == "email" {
                    account.id.clone()
                } else {
                    r.profile_name(&account.kind, &account.id)
                        .map(str::to_owned)
                        .or(e.name.map(|(_, n)| n))
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| account.id.clone())
                };
                let color = hash_color(email.as_deref().unwrap_or(&account.id));
                let terms = [Some(account.id.to_lowercase()), email].into_iter().flatten().collect();
                rows.push(Row {
                    key,
                    id: None,
                    name,
                    role: None,
                    color,
                    known: false,
                    account: Some(account),
                    terms,
                    group: "unknown",
                    total,
                    topics,
                    weeks,
                    last_at,
                    last,
                    last_source,
                    next_meeting: e.meeting,
                });
            }
        }
    }
    // Default order (Last signal): most recent first, then by name.
    rows.sort_by(|a, b| {
        b.last_at
            .cmp(&a.last_at)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(rows)
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "index" => Route::Read(|r, ctx, _args: &Value| Ok(json!(index(r, &ctx.settings.people, ctx.clock)?))),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

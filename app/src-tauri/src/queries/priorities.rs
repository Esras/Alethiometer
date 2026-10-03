//! Priorities (8b): urgent × important, TaskWarrior-style. Four fixed quadrants
//! (Now-ish, Make room for, Quick touch, Can wait). Each row carries its score and
//! the reasons behind it as red/yellow/green pills (how much each reason adds).
//!
//! Quadrant, score and reasons live in the `priorities` table. One kind of reason
//! is computed live at read time: `{"live": "meeting"}` becomes "meeting in 8m"
//! from the clock and the item's linked meeting, and drops once the meeting ends.
//!
//! Work only: items from personal sources never appear here (boundary rule 3).

use super::{dur_label, parse_ts, when_label, Pill, TopicRef};
use crate::api::unknown;
use crate::error::Result;
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const QUADRANTS: [&str; 4] = ["now", "make_room", "quick", "can_wait"];

/// Stored reason. `live` marks a reason whose label is computed at read time.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Reason {
    #[serde(default)]
    label: String,
    #[serde(default)]
    weight: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    live: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub item_id: i64,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub topic: Option<TopicRef>,
    pub reasons: Vec<Pill>,
    pub score: f64,
    /// Time part of the Selected bar meta ("11:00–11:45", "due Wed", "Today 09:12").
    pub when: Option<String>,
    /// People part of the meta (attendees or author).
    pub people: Option<String>,
    pub summary: Option<String>,
    pub body: String,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Quadrant {
    pub key: &'static str,
    pub items: Vec<Row>,
}

pub fn call(app: &App, name: &str, _args: &Value) -> Result<Value> {
    match name {
        "list" => Ok(json!({ "quadrants": list(&app.db(), app.clock.now())? })),
        _ => Err(unknown(name)),
    }
}

/// All four quadrants, always in the same order, rows by score descending.
pub fn list(conn: &Connection, now: NaiveDateTime) -> Result<Vec<Quadrant>> {
    let mut stmt = conn.prepare(
        "SELECT p.item_id, p.quadrant, p.score, p.reasons_json,
                i.title, i.summary, i.body, i.url, i.occurred_at, i.due_at, i.kind, s.kind,
                per.display_name, e.start, e.end, e.attendees,
                (SELECT tl.topic_id FROM topic_links tl
                  WHERE tl.item_id = i.id AND tl.state != 'wrong'
                  ORDER BY tl.state = 'confirmed' DESC LIMIT 1)
           FROM priorities p
           JOIN items i ON i.id = p.item_id
           JOIN sources s ON s.id = i.source_id
           LEFT JOIN people per ON per.id = i.author_person_id
           LEFT JOIN events e ON e.item_id = i.id
          WHERE s.tag = 'work' AND i.status != 'done'
          ORDER BY p.score DESC, i.id",
    )?;
    struct Raw {
        id: i64,
        quadrant: String,
        score: f64,
        reasons: String,
        title: String,
        summary: Option<String>,
        body: String,
        url: Option<String>,
        at: String,
        due: Option<String>,
        kind: String,
        source: String,
        author: Option<String>,
        ev_start: Option<String>,
        ev_end: Option<String>,
        attendees: Option<String>,
        topic_id: Option<i64>,
    }
    let raws = stmt
        .query_map([], |r| {
            Ok(Raw {
                id: r.get(0)?,
                quadrant: r.get(1)?,
                score: r.get(2)?,
                reasons: r.get(3)?,
                title: r.get(4)?,
                summary: r.get(5)?,
                body: r.get(6)?,
                url: r.get(7)?,
                at: r.get(8)?,
                due: r.get(9)?,
                kind: r.get(10)?,
                source: r.get(11)?,
                author: r.get(12)?,
                ev_start: r.get(13)?,
                ev_end: r.get(14)?,
                attendees: r.get(15)?,
                topic_id: r.get(16)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut out: Vec<Quadrant> = QUADRANTS.iter().map(|&key| Quadrant { key, items: vec![] }).collect();
    for r in raws {
        let Some(q) = out.iter_mut().find(|q| q.key == r.quadrant) else { continue };
        let topic = r.topic_id.map(|id| super::topic_ref(conn, id)).transpose()?;
        let stored: Vec<Reason> = serde_json::from_str(&r.reasons).unwrap_or_default();
        let mut reasons = vec![];
        for reason in stored {
            match reason.live.as_deref() {
                Some("meeting") => {
                    if let Some(p) = meeting_pill(conn, r.id, r.topic_id, now)? {
                        reasons.push(p);
                    }
                }
                _ => reasons.push(Pill { label: reason.label, weight: reason.weight }),
            }
        }

        let start = r.ev_start.as_deref().and_then(parse_ts);
        let end = r.ev_end.as_deref().and_then(parse_ts);
        let when = match (start, end) {
            (Some(s), Some(e)) if s.date() == now.date() => Some(format!("{}–{}", s.format("%H:%M"), e.format("%H:%M"))),
            (Some(s), Some(e)) => Some(format!("{}–{}", when_label(s, now, true), e.format("%H:%M"))),
            _ => match r.due.as_deref().and_then(parse_ts) {
                Some(d) => Some(format!("due {}", when_label(d, now, false))),
                None => parse_ts(&r.at).map(|t| when_label(t, now, matches!(r.kind.as_str(), "email" | "message" | "thread"))),
            },
        };
        let people = r.attendees.filter(|a| !a.is_empty()).or(r.author);

        q.items.push(Row {
            item_id: r.id,
            source: r.source,
            kind: r.kind,
            title: r.title,
            topic,
            reasons,
            score: r.score,
            when,
            people,
            summary: r.summary,
            body: r.body,
            url: r.url,
        });
    }
    Ok(out)
}

/// "meeting in 8m" for the item's own meeting, or else the next meeting linked
/// to its topic. Red within 15 minutes, yellow within the hour, green later.
/// None once the meeting is over (or there isn't one today).
fn meeting_pill(conn: &Connection, item_id: i64, topic_id: Option<i64>, now: NaiveDateTime) -> Result<Option<Pill>> {
    let now_s = crate::seed::fmt(now);
    let today = now.date().format("%Y-%m-%d").to_string();
    let next: Option<(String, String)> = conn
        .query_row(
            "SELECT e.start, e.end FROM events e
              WHERE e.is_life_busy = 0 AND e.end > ?1 AND e.start >= ?2 AND e.start < date(?2, '+1 day')
                AND (e.item_id = ?3 OR e.item_id IN
                     (SELECT tl.item_id FROM topic_links tl WHERE tl.topic_id = ?4 AND tl.state != 'wrong'))
              ORDER BY e.item_id = ?3 DESC, e.start LIMIT 1",
            params![now_s, today, item_id, topic_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let Some((start, _)) = next else { return Ok(None) };
    let Some(start) = parse_ts(&start) else { return Ok(None) };
    let secs = (start - now).num_seconds();
    if secs <= 0 {
        return Ok(Some(Pill { label: "meeting now".into(), weight: "red".into() }));
    }
    let mins = (secs + 59) / 60;
    let weight = match mins {
        0..=15 => "red",
        16..=60 => "yellow",
        _ => "green",
    };
    Ok(Some(Pill { label: format!("meeting in {}", dur_label(mins)), weight: weight.into() }))
}

// ---------------------------------------------------------------------------
// Sample data (work only).

struct P<'a> {
    source: &'static str,
    kind: &'static str,
    title: &'a str,
    topic: &'a str,
    quadrant: &'static str,
    score: f64,
    reasons: &'a [(&'a str, &'a str)],
    author: Option<&'static str>,
    summary: &'a str,
    body: &'a str,
    days: i64,
    hm: &'a str,
    due: Option<(i64, &'a str)>,
    url: &'a str,
}

fn reasons_json(reasons: &[(&str, &str)]) -> String {
    let v: Vec<Reason> = reasons
        .iter()
        .map(|(label, weight)| Reason { label: (*label).into(), weight: (*weight).into(), live: None })
        .collect();
    serde_json::to_string(&v).expect("reasons json")
}

fn prioritize(ctx: &Ctx, item_id: i64, quadrant: &str, score: f64, reasons: &str) {
    ctx.conn
        .execute(
            "INSERT INTO priorities (item_id, quadrant, score, reasons_json) VALUES (?1, ?2, ?3, ?4)",
            params![item_id, quadrant, score, reasons],
        )
        .expect("seed priority");
}

pub fn seed(ctx: &Ctx) -> anyhow::Result<()> {
    let c = ctx.conn;

    // Now-ish #1 is today's 11:00 Q4 headcount meeting (already on the strip). The
    // decision it exists for is the item's title; the strip keeps the event title.
    let q4: Option<i64> = c
        .query_row(
            "SELECT item_id FROM events WHERE title = 'Q4 headcount' AND substr(start, 1, 10) = ?1",
            [ctx.today.format("%Y-%m-%d").to_string()],
            |r| r.get(0),
        )
        .ok();
    if let Some(q4) = q4 {
        c.execute(
            "UPDATE items SET title = 'Platform vs Data reqs',
                    body = 'Two open reqs for Q4, one budget line. Priya wants a Data eng hire for the ingest backlog; Jordan wants a second Platform SRE for on-call load. Lee brings attrition numbers. You make the call in the room.',
                    url = 'https://calendar.google.com/calendar/r/day'
              WHERE id = ?1",
            [q4],
        )?;
        let reasons = serde_json::to_string(&[
            Reason { label: String::new(), weight: "red".into(), live: Some("meeting".into()) },
            Reason { label: "your decision".into(), weight: "yellow".into(), live: None },
        ])?;
        prioritize(ctx, q4, "now", 8.4, &reasons);
    }

    let rows: &[P] = &[
        // ---- Now-ish
        P {
            source: "gmail", kind: "email", title: "Reply to Dana", topic: "Acme outage", quadrant: "now", score: 9.1,
            reasons: &[("customer asking", "red"), ("Sev 2", "yellow"), ("today", "green")],
            author: Some("Dana"), summary: "Asking for an ETA and whether webhooks were dropped or delayed",
            body: "Dana (Acme VP Eng): \"Our ops team is seeing delayed webhook deliveries since this morning. Can you confirm whether anything was dropped, and give us an ETA we can pass on?\"",
            days: 0, hm: "09:41", due: None, url: "https://mail.google.com/mail/u/0/#inbox/acme-dana",
        },
        P {
            source: "gmail", kind: "email", title: "Approve Sr. EM offer", topic: "Sr. EM offer", quadrant: "now", score: 7.0,
            reasons: &[("expires Wed", "yellow"), ("hiring", "green")],
            author: Some("Lee"), summary: "Offer letter ready; needs your approval before the candidate's deadline",
            body: "Lee: \"Comp is at band midpoint plus sign-on, as discussed. Candidate has a competing offer that expires Wednesday. Approve in Workday when you can.\"",
            days: -1, hm: "16:20", due: Some((3, "17:00")), url: "https://mail.google.com/mail/u/0/#inbox/sr-em-offer",
        },
        // ---- Make room for
        P {
            source: "sheets", kind: "sheet", title: "Datadog counter-offer", topic: "Datadog renewal", quadrant: "make_room", score: 5.8,
            reasons: &[("$ at stake", "red"), ("due Wed", "yellow")],
            author: Some("Ana"), summary: "Counter to 12% off / 3y: 2y + true-down at 12 months",
            body: "Cost model v3 has the 1y vs 3y tabs. Counter needs the walk-away number agreed with Kim first; send to Ravi before Thursday's final terms call.",
            days: -2, hm: "15:10", due: Some((3, "12:00")), url: "https://docs.google.com/spreadsheets/d/datadog-counter",
        },
        P {
            source: "obsidian", kind: "note", title: "Q4 strategy doc, first draft", topic: "Q4 planning", quadrant: "make_room", score: 4.0,
            reasons: &[("flagged important", "red"), ("no date", "green")],
            author: None, summary: "Outline exists; needs the platform bets section",
            body: "Outline: 1) where we are, 2) three platform bets (ingest v2, alert hygiene, webhook reliability), 3) what we stop doing, 4) headcount ask. Sections 2–4 still empty.",
            days: -5, hm: "08:30", due: None, url: "obsidian://open?vault=work&file=Planning/Q4%20strategy",
        },
        P {
            source: "obsidian", kind: "note", title: "Director narrative, one page", topic: "Career", quadrant: "make_room", score: 2.9,
            reasons: &[("Jobs category", "yellow")],
            author: None, summary: "One-pager for the director promo packet",
            body: "Scope grown this year: platform + data ingest, 3 teams, Acme escalation owner. Needs two concrete org-level outcomes and one miss with what changed.",
            days: -9, hm: "20:00", due: None, url: "obsidian://open?vault=work&file=Career/Director%20narrative",
        },
        // ---- Quick touch (or hand off)
        P {
            source: "jira", kind: "issue", title: "PLAT-1201 review request", topic: "Webhook reliability", quadrant: "quick", score: 5.5,
            reasons: &[("waiting 2d", "yellow"), ("others can review", "green")],
            author: Some("Marco"), summary: "Retry backoff with jitter for v2 delivery workers",
            body: "Marco: \"Adds exponential backoff with full jitter and a per-endpoint retry budget. Would like your eyes on the budget defaults, or hand to Jordan.\"",
            days: -2, hm: "11:05", due: None, url: "https://jira.example.com/browse/PLAT-1201",
        },
        P {
            source: "slack", kind: "message", title: "Jordan: own disk-pressure tuning?", topic: "Alerting · P2", quadrant: "quick", score: 4.7,
            reasons: &[("fires every 20m", "yellow"), ("hand off", "green")],
            author: Some("Jordan"), summary: "Offered to take the node-7 alert tuning",
            body: "Jordan: \"I can take disk-pressure on node-7 this sprint if you're OK with me muting it for 48h while I tune thresholds.\"",
            days: 0, hm: "10:02", due: None, url: "https://slack.com/app_redirect?channel=D-jordan",
        },
        P {
            source: "jira", kind: "issue", title: "PROC-412 sign-off", topic: "Datadog renewal", quadrant: "quick", score: 4.2,
            reasons: &[("blocking Finance", "yellow"), ("one click", "green")],
            author: Some("Kim"), summary: "Budget owner approval on the renewal ticket",
            body: "Kim: \"Finance can't route the PO until the budget owner approves PROC-412. Terms can still change; this is only the budget line.\"",
            days: -1, hm: "14:40", due: None, url: "https://jira.example.com/browse/PROC-412",
        },
        // ---- Can wait
        P {
            source: "gmail", kind: "email", title: "CFP deadline Nov 15", topic: "Conference CFP", quadrant: "can_wait", score: 2.4,
            reasons: &[("7 weeks out", "green")], author: None, summary: "SREcon CFP closes Nov 15",
            body: "Reminder from the program committee: talk proposals close November 15.", days: -6, hm: "08:00", due: None,
            url: "https://mail.google.com/mail/u/0/#inbox/cfp",
        },
        P {
            source: "obsidian", kind: "note", title: "Talk abstract outline", topic: "Conference CFP", quadrant: "can_wait", score: 1.6,
            reasons: &[("no date", "green")], author: None, summary: "\"Retry storms we caused ourselves\"",
            body: "Hook: the Acme retry storm. Three lessons, one diagram, live demo optional.", days: -3, hm: "21:00", due: None,
            url: "obsidian://open?vault=work&file=Talks/Retry%20storms",
        },
        P {
            source: "obsidian", kind: "note", title: "Skip-level notes for Lee", topic: "Career", quadrant: "can_wait", score: 2.2,
            reasons: &[("next month", "green")], author: None, summary: "Topics for the October skip-level",
            body: "Ask about director calibration timeline; bring the on-call load data.", days: -4, hm: "17:30", due: None,
            url: "obsidian://open?vault=work&file=Career/Skip-level",
        },
        P {
            source: "notion", kind: "doc", title: "Read \"Staff to Director\" doc", topic: "Career", quadrant: "can_wait", score: 1.2,
            reasons: &[("reading", "green")], author: Some("Lee"), summary: "Internal leveling guide",
            body: "HR's leveling guide for the director track, shared by Lee.", days: -8, hm: "10:00", due: None,
            url: "https://notion.so/leveling-director",
        },
        P {
            source: "jira", kind: "issue", title: "V1-88 deprecation banner copy", topic: "v1 ingest deprecation", quadrant: "can_wait", score: 2.3,
            reasons: &[("before sunset", "green")], author: Some("Marco"), summary: "Copy for the in-app banner",
            body: "Needs final wording and the sunset date.", days: -5, hm: "13:15", due: None,
            url: "https://jira.example.com/browse/V1-88",
        },
        P {
            source: "sheets", kind: "sheet", title: "Customers still on v1", topic: "v1 ingest deprecation", quadrant: "can_wait", score: 1.9,
            reasons: &[("12 accounts", "green")], author: Some("Sam"), summary: "Account list with v1 traffic share",
            body: "12 accounts still send >10% of traffic to v1. Sam owns outreach.", days: -7, hm: "09:30", due: None,
            url: "https://docs.google.com/spreadsheets/d/v1-customers",
        },
        P {
            source: "notion", kind: "doc", title: "Sunset comms plan", topic: "v1 ingest deprecation", quadrant: "can_wait", score: 1.4,
            reasons: &[("draft", "green")], author: None, summary: "Timeline and who tells whom",
            body: "T-90 email, T-30 banner, T-7 account calls.", days: -12, hm: "15:00", due: None,
            url: "https://notion.so/v1-sunset-comms",
        },
        P {
            source: "sheets", kind: "sheet", title: "Q3 pager stats", topic: "On-call load", quadrant: "can_wait", score: 2.0,
            reasons: &[("for Q4 review", "green")], author: Some("Jordan"), summary: "Pages per shift, by service",
            body: "Pages per shift up 30% vs Q2; alerting noise is most of it.", days: -3, hm: "11:00", due: None,
            url: "https://docs.google.com/spreadsheets/d/pager-q3",
        },
        P {
            source: "jira", kind: "issue", title: "PLAT-1150 runbook cleanup", topic: "On-call load", quadrant: "can_wait", score: 1.1,
            reasons: &[("backlog", "green")], author: None, summary: "Stale runbooks for retired services",
            body: "Seven runbooks point at services that no longer exist.", days: -20, hm: "10:00", due: None,
            url: "https://jira.example.com/browse/PLAT-1150",
        },
        P {
            source: "notion", kind: "doc", title: "Alert ownership map", topic: "Alerting · P2", quadrant: "can_wait", score: 1.7,
            reasons: &[("nice to have", "green")], author: None, summary: "Who owns which alert",
            body: "Half-filled table of alerts to owning team.", days: -14, hm: "16:00", due: None,
            url: "https://notion.so/alert-ownership",
        },
        P {
            source: "github", kind: "pr", title: "Retry budget RFC comments", topic: "Webhook reliability", quadrant: "can_wait", score: 2.1,
            reasons: &[("FYI", "green")], author: Some("Marco"), summary: "platform/rfcs#41",
            body: "Discussion on per-endpoint retry budgets; no decision needed from you yet.", days: -1, hm: "18:10", due: None,
            url: "https://github.com/platform/rfcs/pull/41",
        },
        P {
            source: "slack", kind: "message", title: "Collect team OKR drafts", topic: "Q4 planning", quadrant: "can_wait", score: 1.8,
            reasons: &[("next week", "green")], author: Some("Priya"), summary: "#eng-leads thread",
            body: "Priya: \"Drafts in the shared folder by next Friday please.\"", days: -2, hm: "09:20", due: None,
            url: "https://slack.com/app_redirect?channel=eng-leads",
        },
        P {
            source: "obsidian", kind: "note", title: "Postmortem skeleton", topic: "Acme outage", quadrant: "can_wait", score: 1.5,
            reasons: &[("after fix", "green")], author: None, summary: "Start once the fix ships",
            body: "Timeline, contributing factors, retry storm graph, action items.", days: 0, hm: "09:30", due: None,
            url: "obsidian://open?vault=work&file=Postmortems/Acme",
        },
        P {
            source: "notion", kind: "doc", title: "Log retention tiers", topic: "Datadog renewal", quadrant: "can_wait", score: 1.3,
            reasons: &[("research", "green")], author: Some("Ana"), summary: "15d vs 30d cost comparison",
            body: "Ana's notes on moving debug logs to 15-day retention.", days: -9, hm: "14:00", due: None,
            url: "https://notion.so/log-retention",
        },
        P {
            source: "gmail", kind: "email", title: "Req template for Data eng", topic: "Q4 headcount", quadrant: "can_wait", score: 1.0,
            reasons: &[("after decision", "green")], author: Some("Lee"), summary: "Template to fill if the Data req wins",
            body: "Lee: \"If you go with the Data req, fill this in and I'll open it the same day.\"", days: -1, hm: "12:10", due: None,
            url: "https://mail.google.com/mail/u/0/#inbox/req-template",
        },
        P {
            source: "obsidian", kind: "note", title: "Sr. EM onboarding plan", topic: "Sr. EM offer", quadrant: "can_wait", score: 0.9,
            reasons: &[("after accept", "green")], author: None, summary: "First 30 days",
            body: "Week 1 shadowing, week 2 1:1s with each lead, week 3 own the platform standup.", days: -4, hm: "19:00", due: None,
            url: "obsidian://open?vault=work&file=Hiring/Sr%20EM%20onboarding",
        },
    ];

    for p in rows {
        let id = ctx.item(ItemSeed {
            source: p.source,
            kind: p.kind,
            title: p.title.into(),
            body: p.body.into(),
            summary: Some(p.summary.into()),
            url: Some(p.url.into()),
            author: p.author,
            occurred_at: ctx.at(p.days, p.hm),
            due_at: p.due.map(|(d, hm)| ctx.at(d, hm)),
            owner: Some("you".into()),
            ..Default::default()
        });
        ctx.link(id, p.topic, "confirmed");
        prioritize(ctx, id, p.quadrant, p.score, &reasons_json(p.reasons));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn setup() -> (Connection, NaiveDate, tempfile::TempDir) {
        let conn = crate::db::open_in_memory().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::seed::seed(&conn, day, dir.path()).unwrap();
        (conn, day, dir)
    }

    fn at(day: NaiveDate, hm: &str) -> NaiveDateTime {
        day.and_time(NaiveTime::parse_from_str(hm, "%H:%M").unwrap())
    }

    fn find<'a>(qs: &'a [Quadrant], title: &str) -> &'a Row {
        qs.iter().flat_map(|q| &q.items).find(|r| r.title == title).unwrap_or_else(|| panic!("no row {title}"))
    }

    #[test]
    fn four_quadrants_in_order_sorted_by_score() {
        let (conn, day, _d) = setup();
        let qs = list(&conn, at(day, "10:52")).unwrap();
        assert_eq!(qs.iter().map(|q| q.key).collect::<Vec<_>>(), QUADRANTS);
        let now: Vec<_> = qs[0].items.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(now, ["Reply to Dana", "Platform vs Data reqs", "Approve Sr. EM offer"]);
        for q in &qs {
            assert!(q.items.windows(2).all(|w| w[0].score >= w[1].score), "{} not sorted", q.key);
        }
        assert!(qs[3].items.len() >= 15);
        let dana = find(&qs, "Reply to Dana");
        assert_eq!(dana.topic.as_ref().unwrap().name, "Acme outage");
        assert_eq!(dana.people.as_deref(), Some("Dana"));
    }

    #[test]
    fn meeting_pill_is_live() {
        let (conn, day, _d) = setup();
        let row = |hm: &str| {
            let qs = list(&conn, at(day, hm)).unwrap();
            let r = find(&qs, "Platform vs Data reqs");
            (r.reasons.iter().map(|p| (p.label.clone(), p.weight.clone())).collect::<Vec<_>>(), r.when.clone())
        };
        let (pills, when) = row("10:52");
        assert_eq!(pills[0], ("meeting in 8m".to_string(), "red".to_string()));
        assert_eq!(pills[1].0, "your decision");
        assert_eq!(when.as_deref(), Some("11:00–11:45"));
        assert_eq!(row("10:30").0[0], ("meeting in 30m".to_string(), "yellow".to_string()));
        assert_eq!(row("11:10").0[0].0, "meeting now");
        let (after, _) = row("12:00");
        assert_eq!(after.len(), 1, "meeting pill drops once the meeting is over");
    }

    #[test]
    fn personal_and_done_items_are_left_out() {
        let (conn, day, _d) = setup();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at)
             VALUES ((SELECT id FROM sources WHERE kind = 'marvin'), 'x1', 'task', 'Personal thing', '2026-09-24T09:00:00')",
            [],
        )
        .unwrap();
        let personal = conn.last_insert_rowid();
        conn.execute("INSERT INTO priorities (item_id, quadrant, score) VALUES (?1, 'now', 10.0)", [personal]).unwrap();
        conn.execute("UPDATE items SET status = 'done' WHERE title = 'Approve Sr. EM offer'", []).unwrap();
        let qs = list(&conn, at(day, "10:52")).unwrap();
        let titles: Vec<_> = qs.iter().flat_map(|q| &q.items).map(|r| r.title.as_str()).collect();
        assert!(!titles.contains(&"Personal thing"));
        assert!(!titles.contains(&"Approve Sr. EM offer"));
    }

    #[test]
    fn call_routes_list() {
        let (conn, _day, dir) = setup();
        let app = App::from_parts(conn, crate::settings::Settings::default(), dir.path()).unwrap();
        let v = crate::api::call(&app, "priorities.list", &json!({})).unwrap();
        assert_eq!(v["quadrants"].as_array().unwrap().len(), 4);
        assert!(crate::api::call(&app, "priorities.nope", &json!({})).is_err());
    }
}

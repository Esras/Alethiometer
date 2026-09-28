//! Incoming (8a): the triage queue. Topics with new signals on the left,
//! Unsorted last; the selected topic's new signals grouped by app on the right.
//!
//! - A new signal is an unseen item (`seen = 0`) with a `suggested` link.
//! - Unsorted is every unseen item with no link other than `wrong`.
//! - Confirm: link -> `confirmed`, item seen.
//! - Wrong topic: link -> `wrong`. Nothing else: no reason asked, nothing
//!   deleted, the item stays unseen and so drops into Unsorted (rule 5).
//! - Move (`t`): the user picks a topic; that link becomes `confirmed`, the item
//!   is seen, and the suggestion it came from (if any) is marked `wrong`.
//! - Split (Unsorted only, `t` with messages selected): the selected messages
//!   move to a child item (`split.rs`) confirmed to the picked topic, as `t`
//!   does for a whole item; the rest stays in Unsorted.

use super::{arg_i64, opt_i64, parse_ts, when_label};
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize, Debug)]
pub struct Message {
    /// `item_messages.id`: what a split names.
    pub id: i64,
    pub time: String,
    pub author: String,
    pub text: String,
}

#[derive(Serialize, Debug)]
pub struct Signal {
    pub item_id: i64,
    pub source: String,
    pub kind: String,
    pub title: String,
    /// Muted line next to the title: "42 new · 09:05–10:40", "Sam · 10:41".
    pub meta: String,
    /// One-line context shown when expanded and there are no messages.
    pub summary: Option<String>,
    pub url: Option<String>,
    /// Messages behind the signal (e.g. total in a Slack thread since you last looked).
    pub msg_count: Option<i64>,
    /// Stored context messages, oldest first.
    pub messages: Vec<Message>,
    #[serde(skip)]
    at: String,
}

#[derive(Serialize, Debug)]
pub struct TopicSignals {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub signals: Vec<Signal>,
}

#[derive(Serialize, Debug)]
pub struct Queue {
    pub topics: Vec<TopicSignals>,
    pub unsorted: Vec<Signal>,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let now = app.clock.now();
    let conn = app.db();
    match name {
        "list" => Ok(json!(queue(&conn, now)?)),
        "confirm" => {
            confirm(&conn, arg_i64(args, "item_id")?, arg_i64(args, "topic_id")?)?;
            Ok(json!({ "ok": true }))
        }
        "wrong" => {
            wrong(&conn, arg_i64(args, "item_id")?, arg_i64(args, "topic_id")?)?;
            Ok(json!({ "ok": true }))
        }
        "move" => {
            move_to(&conn, arg_i64(args, "item_id")?, arg_i64(args, "topic_id")?, opt_i64(args, "from_topic_id"))?;
            Ok(json!({ "ok": true }))
        }
        "split" => {
            let ids: Vec<i64> = args
                .get("message_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let child = split_to(&conn, arg_i64(args, "item_id")?, &ids, arg_i64(args, "topic_id")?)?;
            Ok(json!({ "ok": true, "item_id": child }))
        }
        _ => Err(unknown(name)),
    }
}

const SIGNAL_COLS: &str = "i.id, s.kind, i.kind, i.title, i.summary, i.url, i.occurred_at, i.ended_at, i.msg_count,
                           (SELECT display_name FROM people p WHERE p.id = i.author_person_id)";

fn row_to_signal(r: &rusqlite::Row, now: NaiveDateTime) -> rusqlite::Result<Signal> {
    let title: String = r.get(3)?;
    let at: String = r.get(6)?;
    let ended: Option<String> = r.get(7)?;
    let msg_count: Option<i64> = r.get(8)?;
    let author: Option<String> = r.get(9)?;
    Ok(Signal {
        meta: meta(&title, &at, ended.as_deref(), msg_count, author.as_deref(), now),
        item_id: r.get(0)?,
        source: r.get(1)?,
        kind: r.get(2)?,
        title,
        summary: r.get(4)?,
        url: r.get(5)?,
        msg_count,
        messages: vec![],
        // Last activity: a thread counts from its latest message.
        at: ended.clone().unwrap_or_else(|| at.clone()),
    })
}

/// "HH:MM" today, otherwise "Thu 14:00" / "Sep 25".
fn time_label(t: NaiveDateTime, now: NaiveDateTime) -> String {
    if t.date() == now.date() {
        t.format("%H:%M").to_string()
    } else {
        when_label(t, now, true)
    }
}

fn meta(title: &str, at: &str, ended: Option<&str>, msgs: Option<i64>, author: Option<&str>, now: NaiveDateTime) -> String {
    let Some(start) = parse_ts(at) else { return String::new() };
    let end = ended.and_then(parse_ts);
    match (msgs, end) {
        (Some(n), Some(e)) if n > 1 => {
            let range = if e.date() == start.date() {
                format!("{}–{}", time_label(start, now), e.format("%H:%M"))
            } else {
                format!("{}–{}", time_label(start, now), time_label(e, now))
            };
            format!("{n} new · {range}")
        }
        _ => match author {
            // Mail and DM titles already name the sender ("Dana · …", "DM · Priya").
            Some(a) if !title.contains(a) => format!("{a} · {}", time_label(start, now)),
            _ => time_label(start, now),
        },
    }
}

fn load_messages(conn: &Connection, sigs: &mut [Signal]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, at, author, text FROM item_messages WHERE item_id = ?1 ORDER BY at, id")?;
    for s in sigs {
        s.messages = stmt
            .query_map([s.item_id], |r| {
                let at: String = r.get(1)?;
                Ok(Message {
                    id: r.get(0)?,
                    time: parse_ts(&at).map(|t| t.format("%H:%M").to_string()).unwrap_or(at),
                    author: r.get(2)?,
                    text: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(())
}

/// Topics with new signals (most recent signal first), then Unsorted.
pub fn queue(conn: &Connection, now: NaiveDateTime) -> Result<Queue> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SIGNAL_COLS}, t.id, t.name, t.category
           FROM topic_links tl
           JOIN items i ON i.id = tl.item_id
           JOIN sources s ON s.id = i.source_id
           JOIN topics t ON t.id = tl.topic_id
          WHERE tl.state = 'suggested' AND i.seen = 0 AND t.archived_at IS NULL AND s.tag = 'work'
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let mut topics: Vec<TopicSignals> = vec![];
    let rows = stmt.query_map([], |r| Ok((row_to_signal(r, now)?, r.get::<_, i64>(10)?, r.get::<_, String>(11)?, r.get::<_, String>(12)?)))?;
    for row in rows {
        let (sig, id, name, category) = row?;
        match topics.iter_mut().find(|t| t.id == id) {
            Some(t) => t.signals.push(sig),
            None => topics.push(TopicSignals { id, name, category, signals: vec![sig] }),
        }
    }
    // Most recent signal first (rows arrive newest first, so push order already is).
    topics.sort_by(|a, b| b.signals[0].at.cmp(&a.signals[0].at));
    for t in &mut topics {
        load_messages(conn, &mut t.signals)?;
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT {SIGNAL_COLS}
           FROM items i JOIN sources s ON s.id = i.source_id
          WHERE i.seen = 0 AND s.tag = 'work'
            AND NOT EXISTS (SELECT 1 FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong')
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let mut unsorted = stmt.query_map([], |r| row_to_signal(r, now))?.collect::<rusqlite::Result<Vec<_>>>()?;
    load_messages(conn, &mut unsorted)?;
    Ok(Queue { topics, unsorted })
}

fn link_state(conn: &Connection, item_id: i64, topic_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT state FROM topic_links WHERE item_id = ?1 AND topic_id = ?2", params![item_id, topic_id], |r| r.get(0))
        .optional()?)
}

pub fn confirm(conn: &Connection, item_id: i64, topic_id: i64) -> Result<()> {
    if link_state(conn, item_id, topic_id)?.is_none() {
        return Err(msg("no suggested link to confirm"));
    }
    conn.execute(
        "UPDATE topic_links SET state = 'confirmed', updated_at = datetime('now') WHERE item_id = ?1 AND topic_id = ?2",
        params![item_id, topic_id],
    )?;
    conn.execute("UPDATE items SET seen = 1 WHERE id = ?1", [item_id])?;
    confirm_series(conn, item_id, topic_id)?;
    mark_read_remote(conn, item_id)?;
    Ok(())
}

/// A recurring event is triaged once: every instance of its series gets the topic.
fn confirm_series(conn: &Connection, item_id: i64, topic_id: i64) -> Result<()> {
    let sibs = "SELECT o.id FROM items i JOIN items o ON o.source_id = i.source_id AND o.id != i.id
          AND json_extract(o.raw_json, '$.recurringEventId') = json_extract(i.raw_json, '$.recurringEventId')
          WHERE i.id = ?1";
    conn.execute(
        &format!(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
             SELECT id, ?2, 'confirmed', 1.0, 'same series' FROM ({sibs}) WHERE true
             ON CONFLICT (item_id, topic_id) DO UPDATE SET state = 'confirmed', updated_at = datetime('now')"
        ),
        params![item_id, topic_id],
    )?;
    conn.execute(&format!("UPDATE items SET seen = 1 WHERE id IN ({sibs})"), [item_id])?;
    Ok(())
}

/// Confirming a Slack signal also marks it read in Slack, through the outbox.
/// Only items the Slack connector stored with a read marker qualify (threads
/// have none; Slack has no API for a thread's read state).
///
/// Split families (`split.rs`): `conversations.mark` up to a ts also marks
/// everything before it, and a run read in Slack closes the parent's item. So
/// while another part of the family is still new, nothing is marked; the last
/// part to leave Incoming marks the conversation up to the family's newest
/// marker.
fn mark_read_remote(conn: &Connection, item_id: i64) -> Result<()> {
    let row: Option<(i64, String, Option<i64>)> = conn
        .query_row(
            "SELECT s.id, s.kind, i.parent_id FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((source_id, kind, parent)) = row else { return Ok(()) };
    if kind != "slack" {
        return Ok(());
    }
    let root = parent.unwrap_or(item_id);
    let family = "SELECT raw_json, seen, id FROM items WHERE id = ?1 OR parent_id = ?1";
    let mut stmt = conn.prepare(family)?;
    let rows = stmt
        .query_map([root], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.iter().any(|(_, seen, id)| *seen == 0 && *id != item_id) {
        return Ok(());
    }
    let mut newest: Option<(f64, String, String)> = None;
    for (raw, _, _) in rows {
        let raw: Value = serde_json::from_str(&raw).unwrap_or_default();
        if let (Some(channel), Some(ts)) = (raw.get("channel").and_then(Value::as_str), raw.get("mark_ts").and_then(Value::as_str)) {
            let n = ts.parse::<f64>().unwrap_or(0.0);
            if newest.as_ref().is_none_or(|b| n > b.0) {
                newest = Some((n, channel.to_owned(), ts.to_owned()));
            }
        }
    }
    if let Some((_, channel, ts)) = newest {
        crate::outbox::enqueue(conn, source_id, "slack.mark_read", &json!({ "channel": channel, "ts": ts }), Some(item_id))?;
    }
    Ok(())
}

/// Manual split from Unsorted: the selected messages go to a new item confirmed
/// to `topic_id` (like `t` on a whole item). Selecting every message is just a
/// move of the whole item. Returns the item that got the topic.
pub fn split_to(conn: &Connection, item_id: i64, message_ids: &[i64], topic_id: i64) -> Result<i64> {
    let mut ids = message_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut all = super::split::message_ids(conn, item_id)?;
    all.sort_unstable();
    let tx = conn.unchecked_transaction()?;
    let target = if ids == all { item_id } else { super::split::split_off(&tx, item_id, &ids)? };
    move_to(&tx, target, topic_id, None)?;
    tx.commit()?;
    Ok(target)
}

/// Only records that the link was wrong. The item stays unseen, so with no
/// other live link it shows up in Unsorted.
pub fn wrong(conn: &Connection, item_id: i64, topic_id: i64) -> Result<()> {
    let n = conn.execute(
        "UPDATE topic_links SET state = 'wrong', updated_at = datetime('now') WHERE item_id = ?1 AND topic_id = ?2",
        params![item_id, topic_id],
    )?;
    if n == 0 {
        return Err(msg("no link to mark wrong"));
    }
    Ok(())
}

/// The user picked the topic, so it's confirmed. If the item came from another
/// topic's suggestion, that suggestion was wrong.
pub fn move_to(conn: &Connection, item_id: i64, topic_id: i64, from: Option<i64>) -> Result<()> {
    if let Some(f) = from.filter(|f| *f != topic_id) {
        wrong(conn, item_id, f)?;
    }
    conn.execute(
        "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'confirmed', 1.0, 'picked by you')
         ON CONFLICT (item_id, topic_id) DO UPDATE SET state = 'confirmed', updated_at = datetime('now')",
        params![item_id, topic_id],
    )?;
    conn.execute("UPDATE items SET seen = 1 WHERE id = ?1", [item_id])?;
    confirm_series(conn, item_id, topic_id)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sample data (work-only). Now is ~10:52 in the mocks.

pub fn seed(ctx: &Ctx) -> anyhow::Result<()> {
    let new = |it: ItemSeed| ctx.item(ItemSeed { seen: false, ..it });
    let suggest = |it: ItemSeed, topic: &str| {
        let id = new(it);
        ctx.link(id, topic, "suggested");
        id
    };
    let acme = "Acme outage";

    // Acme outage: 3 new signals.
    let inc = suggest(
        ItemSeed {
            source: "slack",
            kind: "thread",
            title: "#inc-2291".into(),
            summary: Some("Sev 2 · Acme webhook deliveries delayed".into()),
            url: Some("slack://channel?team=T0&id=C2291".into()),
            occurred_at: ctx.at(0, "09:05"),
            ended_at: Some(ctx.at(0, "10:40")),
            msg_count: Some(42),
            ..Default::default()
        },
        acme,
    );
    for (t, who, text) in [
        ("09:05", "Jordan", "Root cause looks like a retry storm from the v2 delivery workers."),
        ("09:31", "Marco", "Backoff patch is up, needs a second reviewer."),
        ("10:02", "Sam", "Acme is asking for an exec on a call today."),
        ("10:40", "Jordan", "Fix ETA 14:00. Backlog is draining."),
    ] {
        ctx.message(inc, &ctx.at(0, t), who, text);
    }
    suggest(
        ItemSeed {
            source: "gmail",
            kind: "email",
            title: "Dana · \"We need someone senior on a call today\"".into(),
            summary: Some("Acme's VP Eng wants an exec on a call before end of day.".into()),
            url: Some("https://mail.google.com/mail/u/0/#inbox/acme-dana".into()),
            author: Some("Dana"),
            occurred_at: ctx.at(0, "10:22"),
            ..Default::default()
        },
        acme,
    );
    suggest(
        ItemSeed {
            source: "gcal",
            kind: "event",
            title: "Invite · Acme exec call, 16:15–16:45".into(),
            summary: Some("Sam, Dana · tentative on your calendar".into()),
            url: Some("https://calendar.google.com/calendar/r/day".into()),
            author: Some("Sam"),
            occurred_at: ctx.at(0, "10:41"),
            ..Default::default()
        },
        acme,
    );

    // Alerting · P2: the alert mail, grouped per alert rule. Each group is one
    // signal whose msg_count is the number of alert mails behind it (214 total).
    for (rule, count, from, to, sample) in [
        ("disk-pressure · node-7", 168, "00:14", "10:36", "[P2] disk-pressure node-7: /var at 91%"),
        ("queue-lag · ingest-eu", 31, "06:02", "10:31", "[P2] queue-lag ingest-eu: 4m12s behind"),
        ("cert-expiry · api-gw staging", 15, "07:00", "10:00", "[P2] cert-expiry api-gw-staging: 13 days left"),
    ] {
        let id = suggest(
            ItemSeed {
                source: "gmail",
                kind: "thread",
                title: format!("Alertmanager · {rule}"),
                summary: Some(format!("{count} alert mails since midnight, none acknowledged")),
                url: Some("https://mail.google.com/mail/u/0/#label/alerts-p2".into()),
                occurred_at: ctx.at(0, from),
                ended_at: Some(ctx.at(0, to)),
                msg_count: Some(count),
                ..Default::default()
            },
            "Alerting · P2",
        );
        ctx.message(id, &ctx.at(0, from), "alerts", sample);
        ctx.message(id, &ctx.at(0, to), "alerts", sample);
    }

    suggest(
        ItemSeed {
            source: "sheets",
            kind: "sheet",
            title: "Headcount model v2".into(),
            summary: Some("Lee added attrition numbers to the Q4 tab".into()),
            url: Some("https://docs.google.com/spreadsheets/d/headcount-v2".into()),
            author: Some("Lee"),
            occurred_at: ctx.at(0, "10:05"),
            ..Default::default()
        },
        "Q4 headcount",
    );
    suggest(
        ItemSeed {
            source: "jira",
            kind: "issue",
            title: "PLAT-1170 · Move last 3 customers off v1 ingest".into(),
            summary: Some("Priya moved it to In progress · due Oct 15".into()),
            url: Some("https://jira.example.com/browse/PLAT-1170".into()),
            author: Some("Priya"),
            occurred_at: ctx.at(0, "09:40"),
            ..Default::default()
        },
        "v1 ingest deprecation",
    );
    suggest(
        ItemSeed {
            source: "gmail",
            kind: "email",
            title: "Lee · \"Director-track calibration packet\"".into(),
            summary: Some("Self-review section due Friday".into()),
            url: Some("https://mail.google.com/mail/u/0/#inbox/calibration".into()),
            author: Some("Lee"),
            occurred_at: ctx.at(-1, "16:20"),
            ..Default::default()
        },
        "Career",
    );

    // Unsorted: unseen, no topic yet.
    for it in [
        ItemSeed {
            source: "github",
            kind: "pr",
            title: "platform/ingest#482 · Batch dedupe for webhook payloads".into(),
            summary: Some("Review requested by Marco · +312 −48".into()),
            url: Some("https://github.com/platform/ingest/pull/482".into()),
            author: Some("Marco"),
            occurred_at: ctx.at(0, "10:12"),
            ..Default::default()
        },
        ItemSeed {
            source: "jira",
            kind: "issue",
            title: "PLAT-1193 · Flaky webhook retries in staging".into(),
            summary: Some("Assigned to you · Bug · P3".into()),
            url: Some("https://jira.example.com/browse/PLAT-1193".into()),
            author: Some("Jordan"),
            occurred_at: ctx.at(0, "09:48"),
            ..Default::default()
        },
        ItemSeed {
            source: "gmail",
            kind: "email",
            title: "Ana · \"Cloud spend review, October\"".into(),
            summary: Some("Asks each director for a 10-minute slot".into()),
            url: Some("https://mail.google.com/mail/u/0/#inbox/spend".into()),
            author: Some("Ana"),
            occurred_at: ctx.at(0, "08:55"),
            ..Default::default()
        },
        ItemSeed {
            source: "gcal",
            kind: "event",
            title: "Invite · Architecture review, Thu 11:00".into(),
            summary: Some("Jordan · 45m · Platform guild".into()),
            url: Some("https://calendar.google.com/calendar/r/week".into()),
            author: Some("Jordan"),
            occurred_at: ctx.at(0, "08:31"),
            ..Default::default()
        },
        ItemSeed {
            source: "obsidian",
            kind: "note",
            title: "Inbox/1:1 Marco follow-ups".into(),
            summary: Some("3 open lines from this morning's 1:1".into()),
            occurred_at: ctx.at(0, "10:50"),
            ..Default::default()
        },
    ] {
        new(it);
    }
    // An unread DM run that jumps between subjects: the case for a split.
    let dm = new(ItemSeed {
        source: "slack",
        kind: "thread",
        title: "DM · Priya".into(),
        summary: Some("Can you look at the offsite agenda before Thursday?".into()),
        url: Some("slack://user?team=T0&id=U_PRIYA".into()),
        author: Some("Priya"),
        occurred_at: ctx.at(0, "09:12"),
        ended_at: Some(ctx.at(0, "09:20")),
        msg_count: Some(4),
        ..Default::default()
    });
    for (t, text) in [
        ("09:12", "Globex still hasn't scheduled their v1 ingest cutover."),
        ("09:13", "They want to wait until after their code freeze on Oct 20. OK to push PLAT-1170 out?"),
        ("09:16", "Different thing: Lee asked me for data eng hiring numbers for the Q4 model."),
        ("09:20", "Can you look at the offsite agenda before Thursday?"),
    ] {
        ctx.message(dm, &ctx.at(0, t), "Priya", text);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, NaiveDateTime, tempfile::TempDir) {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap().and_hms_opt(10, 52, 0).unwrap();
        crate::seed::seed(&conn, now.date(), dir.path()).unwrap();
        (conn, now, dir)
    }

    fn topic(q: &Queue, name: &str) -> Option<usize> {
        q.topics.iter().position(|t| t.name == name)
    }

    fn unseen(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM items WHERE seen = 0", [], |r| r.get(0)).unwrap()
    }

    fn shown(q: &Queue) -> usize {
        q.topics.iter().map(|t| t.signals.len()).sum::<usize>() + q.unsorted.len()
    }

    #[test]
    fn seed_matches_mock() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let names: Vec<_> = q.topics.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Acme outage", "Alerting · P2", "Q4 headcount", "v1 ingest deprecation", "Career"]);
        let acme = &q.topics[0];
        assert_eq!(acme.signals.len(), 3);
        let slack = acme.signals.iter().find(|s| s.source == "slack").unwrap();
        assert_eq!(slack.meta, "42 new · 09:05–10:40");
        assert_eq!(slack.messages.len(), 4);
        let cal = acme.signals.iter().find(|s| s.source == "gcal").unwrap();
        assert_eq!(cal.meta, "Sam · 10:41");
        let mail = acme.signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail.meta, "10:22");
        let alerts: i64 = q.topics[1].signals.iter().filter_map(|s| s.msg_count).sum();
        assert_eq!(alerts, 214);
        assert_eq!(q.unsorted.len(), 6);
        assert!(q.unsorted.iter().any(|s| s.source == "github"));
        assert!(q.unsorted.iter().any(|s| s.source == "jira"));
        // The tab badge (unseen items) matches what the queue shows.
        assert_eq!(unseen(&conn) as usize, shown(&q));
    }

    #[test]
    fn confirm_marks_seen_and_confirmed() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let t = &q.topics[0];
        let item = t.signals[0].item_id;
        confirm(&conn, item, t.id).unwrap();
        assert_eq!(link_state(&conn, item, t.id).unwrap().as_deref(), Some("confirmed"));
        let q2 = queue(&conn, now).unwrap();
        assert_eq!(q2.topics[topic(&q2, "Acme outage").unwrap()].signals.len(), 2);
        assert!(!q2.unsorted.iter().any(|s| s.item_id == item));
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    #[test]
    fn wrong_drops_to_unsorted_without_deleting() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let career = &q.topics[topic(&q, "Career").unwrap()];
        let item = career.signals[0].item_id;
        let before: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        wrong(&conn, item, career.id).unwrap();
        assert_eq!(link_state(&conn, item, career.id).unwrap().as_deref(), Some("wrong"));
        let q2 = queue(&conn, now).unwrap();
        assert!(topic(&q2, "Career").is_none(), "topic with no signals left drops off the list");
        assert!(q2.unsorted.iter().any(|s| s.item_id == item));
        let after: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
        assert_eq!(unseen(&conn) as usize, shown(&q2));
        assert!(wrong(&conn, item, 9999).is_err());
    }

    #[test]
    fn move_from_unsorted_confirms_picked_topic() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let pr = q.unsorted.iter().find(|s| s.source == "github").unwrap().item_id;
        let target: i64 = conn.query_row("SELECT id FROM topics WHERE name = 'Webhook reliability'", [], |r| r.get(0)).unwrap();
        move_to(&conn, pr, target, None).unwrap();
        assert_eq!(link_state(&conn, pr, target).unwrap().as_deref(), Some("confirmed"));
        let q2 = queue(&conn, now).unwrap();
        assert_eq!(q2.unsorted.len(), 5);
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    fn topic_named(conn: &Connection, name: &str) -> i64 {
        conn.query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    }

    #[test]
    fn split_from_unsorted_confirms_only_the_selection() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let dm = q.unsorted.iter().find(|s| s.title == "DM · Priya").unwrap();
        assert_eq!(dm.messages.len(), 4);
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        let picked = [dm.messages[1].id, dm.messages[0].id, dm.messages[0].id];
        let child = split_to(&conn, dm.item_id, &picked, v1).unwrap();
        assert_ne!(child, dm.item_id);
        assert_eq!(link_state(&conn, child, v1).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, dm.item_id, v1).unwrap(), None);

        let q2 = queue(&conn, now).unwrap();
        assert!(!q2.unsorted.iter().any(|s| s.item_id == child), "the split-off part is filed");
        let rest = q2.unsorted.iter().find(|s| s.item_id == dm.item_id).unwrap();
        let texts: Vec<&str> = rest.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts[0].starts_with("Different thing"));
        assert_eq!(rest.meta, "2 new · 09:16–09:20");
        assert_eq!(unseen(&conn) as usize, shown(&q2));

        // The child shows in the topic's chronology like any confirmed item.
        let n: i64 = conn
            .query_row("SELECT count(*) FROM topic_links WHERE topic_id = ?1 AND item_id = ?2 AND state = 'confirmed'", params![v1, child], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);

        // Selecting everything that's left is a plain move of the item.
        let all: Vec<i64> = rest.messages.iter().map(|m| m.id).collect();
        let q4 = topic_named(&conn, "Q4 headcount");
        assert_eq!(split_to(&conn, dm.item_id, &all, q4).unwrap(), dm.item_id);
        assert_eq!(link_state(&conn, dm.item_id, q4).unwrap().as_deref(), Some("confirmed"));
        // Nothing selected, or someone else's message: refused, nothing changes.
        assert!(split_to(&conn, dm.item_id, &[], q4).is_err());
        let before: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        let other = q2.topics[0].signals.iter().find(|s| !s.messages.is_empty()).unwrap().messages[0].id;
        assert!(split_to(&conn, dm.item_id, &[all[0], other], q4).is_err());
        let after: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn split_family_marks_slack_read_once_all_parts_are_triaged() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let dm = q.unsorted.iter().find(|s| s.title == "DM · Priya").unwrap();
        conn.execute(
            r#"UPDATE items SET raw_json = '{"channel":"D1","mark_ts":"100.4","msgs":["100.1","100.2","100.3","100.4"]}' WHERE id = ?1"#,
            [dm.item_id],
        )
        .unwrap();
        let marks = || -> Vec<String> {
            conn.prepare("SELECT payload_json FROM outbox WHERE op = 'slack.mark_read'")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        // The newest message goes to its own item, suggested to a topic.
        let tx = conn.unchecked_transaction().unwrap();
        let child = super::super::split::split_off(&tx, dm.item_id, &[dm.messages[3].id]).unwrap();
        tx.commit().unwrap();
        let q4 = topic_named(&conn, "Q4 planning");
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", params![child, q4]).unwrap();
        // Confirming it while the older part is still new must not mark the DM
        // read in Slack (that would mark the older messages read too).
        confirm(&conn, child, q4).unwrap();
        assert!(marks().is_empty());
        // Once the rest is triaged, the conversation is marked up to the newest part.
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", params![dm.item_id, v1]).unwrap();
        confirm(&conn, dm.item_id, v1).unwrap();
        let m = marks();
        assert_eq!(m.len(), 1);
        let v: Value = serde_json::from_str(&m[0]).unwrap();
        assert_eq!(v, json!({ "channel": "D1", "ts": "100.4" }));
    }

    #[test]
    fn move_from_suggestion_marks_it_wrong() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let q4 = &q.topics[topic(&q, "Q4 headcount").unwrap()];
        let item = q4.signals[0].item_id;
        let target: i64 = conn.query_row("SELECT id FROM topics WHERE name = 'Q4 planning'", [], |r| r.get(0)).unwrap();
        move_to(&conn, item, target, Some(q4.id)).unwrap();
        assert_eq!(link_state(&conn, item, q4.id).unwrap().as_deref(), Some("wrong"));
        assert_eq!(link_state(&conn, item, target).unwrap().as_deref(), Some("confirmed"));
        // Picking a topic that was once marked wrong turns it back to confirmed.
        let q = queue(&conn, now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let it = acme.signals[0].item_id;
        wrong(&conn, it, acme.id).unwrap();
        move_to(&conn, it, acme.id, None).unwrap();
        assert_eq!(link_state(&conn, it, acme.id).unwrap().as_deref(), Some("confirmed"));
    }
}

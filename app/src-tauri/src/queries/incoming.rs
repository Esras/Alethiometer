//! Incoming (8a): the triage queue. Topics with new signals on the left,
//! Unsorted last; the selected topic's new signals grouped by app on the right.
//!
//! - A new signal is an unseen item (`seen = 0`) with a `suggested` link, or a `confirmed` one (an update).
//! - Unsorted is every unseen item with no link other than `wrong`.
//! - Confirm: link -> `confirmed`, item seen.
//! - Wrong topic: link -> `wrong`. Nothing else: no reason asked, nothing
//!   deleted, the item stays unseen and so drops into Unsorted (rule 5).
//! - Move (`t`): the user picks a topic; that link becomes `confirmed`, the item
//!   is seen, and the suggestion it came from (if any) is marked `wrong`.
//! - Split (Unsorted only, `t` with messages selected): the selected messages
//!   move to a child item (`split.rs`) confirmed to the picked topic, as `t`
//!   does for a whole item; the rest stays in Unsorted.
//! - Split review (15g/15h): a signal the AI split across topics shows a
//!   `split` pill and ⋯ where its other parts' messages came in between.
//!   `family` lists every part still in Incoming with its messages; the review
//!   moves messages between parts (`review_move`, a new topic makes a new part),
//!   dismisses single messages into one seen part of the family
//!   (`review_dismiss`), and confirms every part that has a topic
//!   (`review_confirm`). Parts left empty are pruned (`split::prune`).
//! - Every message carries who wrote it (`people::Who`: person colour, People
//!   note), and a signal from an unknown sender seen often enough carries a
//!   new-person suggestion (17a).

use super::people::{self, Resolver, Suggestion, Who};
use super::{arg_i64, opt_i64, parse_ts, when_label};
use crate::api::unknown;
use crate::settings::PeopleSettings;
use crate::error::{msg, Result};
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize, Debug)]
pub struct Message {
    /// `item_messages.id`: what a split names.
    pub id: i64,
    pub time: String,
    pub author: String,
    pub text: String,
    /// Person colour and People note (16c, 17a).
    pub who: Who,
    /// Messages of another part of this split came in just before this one (⋯).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gap: bool,
    /// The split part holding it (`family` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<i64>,
    /// Thread replies to this message, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Message>,
    /// Already there when you last triaged the item: shown faded.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read: bool,
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
    pub context: Vec<Message>,
    /// Already filed under this topic: new activity, not a suggestion.
    pub update: bool,
    /// A todo waiting for this signal to be confirmed.
    pub held: Option<super::todos::Held>,
    /// One part of a split whose other parts still hold messages (15g).
    pub split: bool,
    /// An unknown sender in this signal worth a People note (17a).
    pub new_person: Option<Suggestion>,
    /// The item's author as the meta line names it, matched like a message's
    /// sender (16c colour, 17b link).
    pub author: Option<Author>,
    /// Gmail's own current label, matched to a category key (`config.category_labels`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip)]
    at: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct Author {
    pub name: String,
    pub who: Who,
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
        "list" => Ok(json!(queue_with(&conn, now, &app.settings().people)?)),
        "family" => Ok(json!(family(&conn, arg_i64(args, "item_id")?, scope_arg(args).as_deref(), &app.settings().people)?)),
        "detail" => Ok(json!(detail(&conn, arg_i64(args, "item_id")?, &app.settings().people)?)),
        "reclassify" => {
            let moved = reclassify(&conn, arg_i64(args, "item_id")?, opt_i64(args, "from_topic_id"), arg_i64(args, "to_topic_id")?)?;
            Ok(json!({ "ok": true, "moved": moved }))
        }
        "unreclassify" => {
            let moved: Vec<Moved> = serde_json::from_value(args.get("moved").cloned().unwrap_or_default()).map_err(|e| msg(format!("bad 'moved': {e}")))?;
            unreclassify(&conn, &moved, opt_i64(args, "from_topic_id"), arg_i64(args, "to_topic_id")?)?;
            Ok(json!({ "ok": true }))
        }
        "review_move" => {
            let to = match (opt_i64(args, "to_item_id"), opt_i64(args, "topic_id")) {
                (Some(part), _) => To::Part(part),
                (None, Some(topic)) => To::Topic(topic),
                // No topic: Unsorted.
                (None, None) => To::Unsorted,
            };
            let part = review_move(&conn, arg_i64(args, "item_id")?, &ids_arg(args), to, scope_arg(args).as_deref())?;
            Ok(json!({ "ok": true, "item_id": part }))
        }
        "review_dismiss" => {
            let part = review_dismiss(&conn, arg_i64(args, "item_id")?, &ids_arg(args))?;
            Ok(json!({ "ok": true, "item_id": part }))
        }
        "review_confirm" => Ok(json!({ "ok": true, "confirmed": review_confirm(&conn, arg_i64(args, "item_id")?)? })),
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
        "dismiss" | "undismiss" => {
            dismiss(&conn, arg_i64(args, "item_id")?, name == "dismiss")?;
            Ok(json!({ "ok": true }))
        }
        "trash" => {
            trash(&conn, arg_i64(args, "item_id")?)?;
            Ok(json!({ "ok": true }))
        }
        "split" => {
            let child = split_to(&conn, arg_i64(args, "item_id")?, &ids_arg(args), arg_i64(args, "topic_id")?)?;
            Ok(json!({ "ok": true, "item_id": child }))
        }
        _ => Err(unknown(name)),
    }
}

/// A split review of filed signals (18c/18d) names the parts it works on; Incoming's doesn't.
fn scope_arg(args: &Value) -> Option<Vec<i64>> {
    args.get("parts").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect())
}

fn ids_arg(args: &Value) -> Vec<i64> {
    args.get("message_ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

const SIGNAL_COLS: &str = "i.id, s.kind, i.kind, i.title, i.summary, i.url, i.occurred_at, i.ended_at, i.msg_count,
                           (SELECT display_name FROM people p WHERE p.id = i.author_person_id), json_extract(i.raw_json, '$.category'),
                           json_extract(i.raw_json, '$.change'), json_extract(i.raw_json, '$.moved_from'), CASE WHEN i.kind = 'event' THEN i.body END";

fn row_to_signal(r: &rusqlite::Row, now: NaiveDateTime) -> rusqlite::Result<Signal> {
    let title: String = r.get(3)?;
    let at: String = r.get(6)?;
    let ended: Option<String> = r.get(7)?;
    let msg_count: Option<i64> = r.get(8)?;
    let author: Option<String> = r.get(9)?;
    let category: Option<String> = r.get(10)?;
    let kind: String = r.get(2)?;
    let event = kind == "event";
    let change: Option<String> = r.get(11)?;
    let moved_from: Option<String> = r.get(12)?;
    let description: Option<String> = r.get(13)?;
    Ok(Signal {
        meta: if event { event_meta(&at, ended.as_deref(), author.as_deref(), now) } else { meta(&title, &at, ended.as_deref(), msg_count, author.as_deref(), now) },
        change: change.as_deref().and_then(|c| change_label(c, moved_from.as_deref(), now)),
        description: description.map(|d| excerpt(d.trim(), 400)).filter(|d| !d.is_empty()),
        item_id: r.get(0)?,
        source: r.get(1)?,
        kind,
        title,
        summary: r.get(4)?,
        url: r.get(5)?,
        msg_count,
        messages: vec![],
        context: vec![],
        update: false,
        held: None,
        split: false,
        new_person: None,
        author: None,
        category,
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

fn day_time(t: NaiveDateTime, now: NaiveDateTime) -> String {
    format!("{} {}", when_label(t, now, false), t.format("%H:%M"))
}

/// "Sam · Thu 11:00–11:45": when the event is, not when it was sent.
fn event_meta(at: &str, ended: Option<&str>, author: Option<&str>, now: NaiveDateTime) -> String {
    let Some(start) = parse_ts(at) else { return String::new() };
    let end = ended.and_then(parse_ts).filter(|e| *e > start);
    let midnight = |t: NaiveDateTime| t.time() == chrono::NaiveTime::MIN;
    let when = match end {
        Some(e) if midnight(start) && midnight(e) => {
            let last = e - chrono::Duration::days(1);
            if last.date() > start.date() {
                format!("{}–{}", when_label(start, now, false), when_label(last, now, false))
            } else {
                when_label(start, now, false)
            }
        }
        Some(e) if e.date() == start.date() => format!("{}–{}", day_time(start, now), e.format("%H:%M")),
        Some(e) => format!("{}–{}", day_time(start, now), day_time(e, now)),
        None => day_time(start, now),
    };
    match author {
        Some(a) => format!("{a} · {when}"),
        None => when,
    }
}

fn change_label(change: &str, moved_from: Option<&str>, now: NaiveDateTime) -> Option<String> {
    Some(match change {
        "invite" => "New invite".into(),
        "new" => "New event".into(),
        "moved" => match moved_from.and_then(parse_ts) {
            Some(t) => format!("Moved from {}", day_time(t, now)),
            None => "Moved".into(),
        },
        "guests" => "Guests changed".into(),
        "updated" => "Updated".into(),
        "cancelled" => "Cancelled".into(),
        _ => return None,
    })
}

fn excerpt(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
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

/// What message rows need besides their own columns: who wrote them, and
/// which unknown senders are worth a People note.
pub struct Lookup {
    pub people: Resolver,
    pub suggest: HashMap<(String, String), Suggestion>,
}

impl Lookup {
    pub fn load(conn: &Connection, settings: &PeopleSettings, now: NaiveDateTime) -> Result<Lookup> {
        let people = Resolver::load(conn, settings)?;
        let suggest = people::suggestions(conn, &people, settings, now)?;
        Ok(Lookup { people, suggest })
    }
}

/// (message, context, ext_key, reply_to, author_id, at) for an item, oldest first.
type MsgRow = (Message, bool, Option<String>, Option<String>, Option<String>, String);

fn message_rows(conn: &Connection, look: &Lookup, source: &str, item_id: i64) -> Result<Vec<MsgRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, at, author, text, context, ext_key, reply_to, author_id, read FROM item_messages WHERE item_id = ?1 ORDER BY at, id",
    )?;
    let rows = stmt
        .query_map([item_id], |r| {
            let at: String = r.get(1)?;
            let author: String = r.get(2)?;
            let author_id: Option<String> = r.get(7)?;
            let m = Message {
                id: r.get(0)?,
                time: parse_ts(&at).map(|t| t.format("%H:%M").to_string()).unwrap_or_else(|| at.clone()),
                who: look.people.who(source, author_id.as_deref(), &author),
                author,
                text: r.get(3)?,
                gap: false,
                part: None,
                replies: vec![],
                read: r.get(8)?,
            };
            Ok((m, r.get::<_, bool>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, Option<String>>(6)?, author_id, at))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Top-level messages with their replies nested, and the context messages.
fn nest(rows: Vec<MsgRow>) -> (Vec<(Message, String)>, Vec<Message>) {
    let (replies, rows): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| r.3.is_some());
    let (context, mut own): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| r.1);
    for r in replies {
        match own.iter_mut().find(|o| o.2 == r.3) {
            Some(parent) => parent.0.replies.push(r.0),
            None => own.push(r),
        }
    }
    (own.into_iter().map(|r| (r.0, r.5)).collect(), context.into_iter().map(|r| r.0).collect())
}

/// Times of the top-level messages the other parts of an item's split family hold.
fn sibling_times(conn: &Connection, item_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "WITH root AS (SELECT coalesce(parent_id, id) AS id FROM items WHERE id = ?1)
         SELECT m.at FROM item_messages m
          WHERE m.item_id IN (SELECT id FROM root UNION SELECT i.id FROM items i, root WHERE i.parent_id = root.id)
            AND m.item_id != ?1 AND m.context = 0 AND m.reply_to IS NULL",
    )?;
    let times = stmt.query_map([item_id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(times)
}

/// The best new-person suggestion among a signal's senders and its author.
fn new_person(conn: &Connection, look: &Lookup, s: &Signal, author_ids: &[(String, String)]) -> Result<Option<Suggestion>> {
    if look.suggest.is_empty() {
        return Ok(None);
    }
    let mut accounts: Vec<(String, String)> = author_ids.iter().filter_map(|(_, h)| people::account_of(&s.source, h)).collect();
    let handle: Option<String> = conn
        .query_row(
            "SELECT h.handle FROM items i JOIN person_handles h ON h.person_id = i.author_person_id AND h.source_kind = ?2 WHERE i.id = ?1",
            params![s.item_id, s.source],
            |r| r.get(0),
        )
        .optional()?;
    accounts.extend(handle.and_then(|h| people::account_of(&s.source, &h)));
    Ok(accounts.iter().filter_map(|a| look.suggest.get(a)).max_by_key(|g| g.signals).cloned())
}

fn item_author(conn: &Connection, look: &Lookup, s: &Signal) -> Result<Option<Author>> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT p.display_name, h.handle FROM items i JOIN people p ON p.id = i.author_person_id
               LEFT JOIN person_handles h ON h.person_id = p.id AND h.source_kind = ?2
              WHERE i.id = ?1",
            params![s.item_id, s.source],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(name, handle)| Author { who: look.people.who(&s.source, handle.as_deref(), &name), name }))
}

fn load_messages(conn: &Connection, look: &Lookup, sigs: &mut [Signal]) -> Result<()> {
    for s in sigs {
        let rows = message_rows(conn, look, &s.source, s.item_id)?;
        let author_ids: Vec<(String, String)> =
            rows.iter().filter(|r| !r.1).filter_map(|r| r.4.clone().map(|id| (r.0.author.clone(), id))).collect();
        let (mut own, context) = nest(rows);
        let others = sibling_times(conn, s.item_id)?;
        s.split = !others.is_empty();
        for i in 1..own.len() {
            let (prev, at) = (own[i - 1].1.clone(), own[i].1.clone());
            own[i].0.gap = others.iter().any(|t| *t > prev && *t < at);
        }
        s.messages = own.into_iter().map(|r| r.0).collect();
        let triaged = s.messages.iter().any(|m| m.read || m.replies.iter().any(|r| r.read));
        let thread: bool = conn.query_row("SELECT json_extract(raw_json, '$.thread_ts') IS NOT NULL FROM items WHERE id = ?1", [s.item_id], |r| r.get(0))?;
        s.context = if triaged && !thread { vec![] } else { context };
        s.held = super::todos::held(conn, s.item_id)?;
        s.new_person = new_person(conn, look, s, &author_ids)?;
        s.author = item_author(conn, look, s)?;
    }
    Ok(())
}

/// Topics with new signals (most recent signal first), then Unsorted.
pub fn queue(conn: &Connection, now: NaiveDateTime) -> Result<Queue> {
    queue_with(conn, now, &PeopleSettings::default())
}

pub fn queue_with(conn: &Connection, now: NaiveDateTime, settings: &PeopleSettings) -> Result<Queue> {
    let look = Lookup::load(conn, settings, now)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {SIGNAL_COLS}, t.id, t.name, t.category, tl.state = 'confirmed'
           FROM topic_links tl
           JOIN items i ON i.id = tl.item_id
           JOIN sources s ON s.id = i.source_id
           JOIN topics t ON t.id = tl.topic_id
          WHERE tl.state IN ('suggested', 'confirmed') AND i.seen = 0 AND t.archived_at IS NULL AND s.tag = 'work'
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let mut topics: Vec<TopicSignals> = vec![];
    let rows = stmt.query_map([], |r| {
        let sig = Signal { update: r.get(17)?, ..row_to_signal(r, now)? };
        Ok((sig, r.get::<_, i64>(14)?, r.get::<_, String>(15)?, r.get::<_, String>(16)?))
    })?;
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
        load_messages(conn, &look, &mut t.signals)?;
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT {SIGNAL_COLS}
           FROM items i JOIN sources s ON s.id = i.source_id
          WHERE i.seen = 0 AND s.tag = 'work'
            AND NOT EXISTS (SELECT 1 FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong')
          ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
    ))?;
    let mut unsorted = stmt.query_map([], |r| row_to_signal(r, now))?.collect::<rusqlite::Result<Vec<_>>>()?;
    load_messages(conn, &look, &mut unsorted)?;
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
    mark_read_remote(conn, item_id, topic_id)?;
    settle_todo(conn, item_id, topic_id)
}

/// A todo held on the signal is created with the confirmed topic. (A Gmail
/// thread leaves the Inbox through filing itself, todo or not: `file_gmail`.)
fn settle_todo(conn: &Connection, item_id: i64, topic_id: i64) -> Result<()> {
    super::todos::settle(conn, item_id, topic_id)?;
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

/// Confirming a Slack or Gmail signal also marks it read there, through the outbox.
/// Only items the Slack connector stored with a read marker qualify (threads
/// have none; Slack has no API for a thread's read state).
///
/// Split families (`split.rs`): `conversations.mark` up to a ts also marks
/// everything before it, and a run read in Slack closes the parent's item. So
/// while another part of the family is still new, nothing is marked; the last
/// part to leave Incoming marks the conversation up to the family's newest
/// marker.
fn mark_read_remote(conn: &Connection, item_id: i64, topic_id: i64) -> Result<()> {
    let Some((source_id, kind, raws)) = settled_family(conn, item_id)? else { return Ok(()) };
    match kind.as_str() {
        "slack" => mark_slack_read(conn, source_id, item_id, &raws),
        "gmail" => file_gmail(conn, source_id, item_id, &raws, topic_id),
        _ => Ok(()),
    }
}

/// Source, kind and every family part's `raw_json`; `None` while another part of the family is still new.
fn settled_family(conn: &Connection, item_id: i64) -> Result<Option<(i64, String, Vec<Value>)>> {
    let row: Option<(i64, String, Option<i64>)> = conn
        .query_row(
            "SELECT s.id, s.kind, i.parent_id FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((source_id, kind, parent)) = row else { return Ok(None) };
    let root = parent.unwrap_or(item_id);
    let family = "SELECT raw_json, seen, id FROM items WHERE id = ?1 OR parent_id = ?1";
    let mut stmt = conn.prepare(family)?;
    let rows = stmt
        .query_map([root], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.iter().any(|(_, seen, id)| *seen == 0 && *id != item_id) {
        return Ok(None);
    }
    let raws = rows.into_iter().map(|(raw, _, _)| serde_json::from_str(&raw).unwrap_or_default()).collect();
    Ok(Some((source_id, kind, raws)))
}

fn mark_slack_read(conn: &Connection, source_id: i64, item_id: i64, raws: &[Value]) -> Result<()> {
    let mut newest: Option<(f64, String, String)> = None;
    for raw in raws {
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

/// The thread, every Gmail message id the user saw in it (so `gmail.file` can spot mail that came later), and the
/// Gmail label of the topic's category (`config.category_labels`), if any.
fn file_gmail(conn: &Connection, source_id: i64, item_id: i64, raws: &[Value], topic_id: i64) -> Result<()> {
    if crate::outbox::read_only(conn, source_id)?.is_some() {
        return Ok(());
    }
    let Some(thread) = raws.iter().find_map(|r| r.get("thread_id").and_then(Value::as_str)) else { return Ok(()) };
    let mut ids: Vec<&str> = vec![];
    for id in raws.iter().filter_map(|r| r.get("msgs").and_then(Value::as_array)).flatten().filter_map(Value::as_str) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let (config, category): (String, String) = conn.query_row(
        "SELECT s.config_json, t.category FROM sources s, topics t WHERE s.id = ?1 AND t.id = ?2",
        params![source_id, topic_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let config: Value = serde_json::from_str(&config).unwrap_or_default();
    // Filing archives the thread, with or without a todo (user, 2026-10-01).
    let mut payload = json!({ "thread_id": thread, "message_ids": ids, "archive": true });
    if let Some(label) = config.pointer(&format!("/category_labels/{category}")).and_then(Value::as_str) {
        payload["label"] = json!(label);
    }
    crate::outbox::enqueue(conn, source_id, "gmail.file", &payload, Some(item_id))?;
    Ok(())
}

fn gmail_op(conn: &Connection, item_id: i64, op: &str) -> Result<()> {
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT s.id, s.kind, i.external_id FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match row {
        Some((source_id, kind, thread)) if kind == "gmail" && crate::outbox::read_only(conn, source_id)?.is_none() => {
            crate::outbox::enqueue(conn, source_id, op, &json!({ "thread_id": thread }), Some(item_id))?;
            Ok(())
        }
        _ => Ok(()),
    }
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

/// Out of triage without a topic; new activity brings it back. The source is only touched for Gmail (`gmail_op`).
pub fn dismiss(conn: &Connection, item_id: i64, on: bool) -> Result<()> {
    if conn.execute("UPDATE items SET seen = ?2 WHERE id = ?1", params![item_id, on])? == 0 {
        return Err(msg("no such item"));
    }
    super::todos::discard(conn, item_id, on)?;
    gmail_op(conn, item_id, if on { "gmail.archive" } else { "gmail.restore" })
}

pub fn trash(conn: &Connection, item_id: i64) -> Result<()> {
    let is_gmail: bool = conn
        .query_row("SELECT s.kind = 'gmail' FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1", [item_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| msg("no such item"))?;
    if !is_gmail {
        return Err(msg("only Gmail threads can be trashed"));
    }
    conn.execute("UPDATE items SET seen = 1 WHERE id = ?1", [item_id])?;
    gmail_op(conn, item_id, "gmail.trash")
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
    if let Some((source_id, kind, raws)) = settled_family(conn, item_id)? {
        if kind == "gmail" {
            file_gmail(conn, source_id, item_id, &raws, topic_id)?;
        }
    }
    settle_todo(conn, item_id, topic_id)
}

// ---------------------------------------------------------------------------
// Split review (15g, 15h).

#[derive(Serialize, Debug)]
pub struct PartTopic {
    pub id: i64,
    pub name: String,
    pub category: String,
}

#[derive(Serialize, Debug)]
pub struct Part {
    pub item_id: i64,
    /// The part's live topic (suggested or confirmed); None is Unsorted.
    pub topic: Option<PartTopic>,
    /// The family's dismissed messages (`review_dismiss`).
    pub dismissed: bool,
}

#[derive(Serialize, Debug)]
pub struct Family {
    pub root: i64,
    pub title: String,
    pub source: String,
    pub url: Option<String>,
    /// `dm` (1:1, opens In order), `group`, `channel`, `thread` or `other`.
    pub conv: &'static str,
    pub parts: Vec<Part>,
    /// Every part's top-level messages in time order, each with its `part`.
    pub messages: Vec<Message>,
}

fn conv_kind(title: &str, raw: &Value) -> &'static str {
    if raw.get("thread_ts").is_some() {
        "thread"
    } else if title.starts_with("DM ·") {
        "dm"
    } else if title.starts_with("Group DM") {
        "group"
    } else if raw.get("channel").is_some() {
        "channel"
    } else {
        "other"
    }
}

fn is_dismissed_part(conn: &Connection, item_id: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT coalesce(json_extract(raw_json, '$.dismissed'), 0) FROM items WHERE id = ?1", [item_id], |r| r.get(0))?)
}

fn live_topic(conn: &Connection, item_id: i64) -> Result<Option<PartTopic>> {
    Ok(conn
        .query_row(
            "SELECT t.id, t.name, t.category FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
              WHERE tl.item_id = ?1 AND tl.state IN ('suggested', 'confirmed') AND t.archived_at IS NULL
              ORDER BY tl.updated_at DESC, tl.rowid DESC LIMIT 1",
            [item_id],
            |r| Ok(PartTopic { id: r.get(0)?, name: r.get(1)?, category: r.get(2)? }),
        )
        .optional()?)
}

/// The parts of `item_id`'s family the review works on: those still in
/// Incoming (or, reviewing filed signals, the parts in `scope`), plus the
/// family's dismissed messages.
fn review_parts(conn: &Connection, item_id: i64, scope: Option<&[i64]>) -> Result<(i64, Vec<Part>)> {
    let (root, ids) = super::split::family(conn, item_id)?;
    let mut parts = vec![];
    for id in ids {
        let (seen, has): (bool, bool) = conn.query_row(
            "SELECT seen, EXISTS (SELECT 1 FROM item_messages WHERE item_id = ?1 AND context = 0) FROM items WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let dismissed = is_dismissed_part(conn, id)?;
        let wanted = match scope {
            Some(parts) => parts.contains(&id),
            None => !seen,
        };
        if (dismissed && has) || (!dismissed && wanted) {
            parts.push(Part { item_id: id, topic: if dismissed { None } else { live_topic(conn, id)? }, dismissed });
        }
    }
    Ok((root, parts))
}

pub fn family(conn: &Connection, item_id: i64, scope: Option<&[i64]>, settings: &PeopleSettings) -> Result<Family> {
    let (root, parts) = review_parts(conn, item_id, scope)?;
    let (title, source, url, raw): (String, String, Option<String>, String) = conn.query_row(
        "SELECT i.title, s.kind, i.url, i.raw_json FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let look = Lookup { people: Resolver::load(conn, settings)?, suggest: HashMap::new() };
    let mut all: Vec<(Message, String)> = vec![];
    for p in &parts {
        let (own, _) = nest(message_rows(conn, &look, &source, p.item_id)?);
        all.extend(own.into_iter().map(|(m, at)| (Message { part: Some(p.item_id), ..m }, at)));
    }
    all.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.id.cmp(&b.0.id)));
    let raw: Value = serde_json::from_str(&raw).unwrap_or_default();
    Ok(Family { root, conv: conv_kind(&title, &raw), title, source, url, parts, messages: all.into_iter().map(|(m, _)| m).collect() })
}

/// Where `review_move` sends messages: another part of the family, a topic
/// (its part in the review, else a new part suggested to it), or Unsorted
/// (the review's part with no topic, else a new one). Undo goes by topic or
/// Unsorted, since the part a message came from may have been pruned.
#[derive(Clone, Copy)]
pub enum To {
    Part(i64),
    Topic(i64),
    Unsorted,
}
/// After a review edit: a part with messages is in Incoming (unless already
/// filed or it holds dismissed messages), an emptied child goes away.
fn settle_parts(conn: &Connection, parts: &[i64]) -> Result<()> {
    for &id in parts {
        let (has, filed): (bool, bool) = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM item_messages WHERE item_id = ?1 AND context = 0),
                    EXISTS (SELECT 1 FROM topic_links WHERE item_id = ?1 AND state = 'confirmed')",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if !has {
            super::split::prune(conn, id)?;
        } else if is_dismissed_part(conn, id)? {
            conn.execute("UPDATE items SET seen = 1 WHERE id = ?1", [id])?;
        } else if !filed {
            conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Move messages of one part to another part, a topic or Unsorted. Returns
/// the part they're in now. Reviewing filed signals (`scope`), a new part is
/// filed straight away (confirmed), not suggested.
pub fn review_move(conn: &Connection, from: i64, ids: &[i64], to: To, scope: Option<&[i64]>) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let target = match to {
        To::Part(part) => part,
        To::Topic(_) | To::Unsorted => {
            let topic = match to {
                To::Topic(t) => Some(t),
                _ => None,
            };
            let (_, parts) = review_parts(&tx, from, scope)?;
            match parts.iter().find(|p| !p.dismissed && p.topic.as_ref().map(|t| t.id) == topic) {
                Some(p) => p.item_id,
                None => {
                    let child = new_part(&tx, from, ids, topic, scope.is_some())?;
                    tx.commit()?;
                    return Ok(child);
                }
            }
        }
    };
    if target != from {
        super::split::move_messages(&tx, from, ids, target)?;
        settle_parts(&tx, &[target, from])?;
    }
    tx.commit()?;
    Ok(target)
}

/// A new part of the family holding `ids`, linked to `topic` (None:
/// Unsorted): suggested in Incoming's review, confirmed (`filed`) when
/// reclassifying a filed signal.
fn new_part(conn: &Connection, from: i64, ids: &[i64], topic: Option<i64>, filed: bool) -> Result<i64> {
    let child = super::split::carve(conn, from, ids, true)?;
    let seen = filed && topic.is_some();
    conn.execute("UPDATE items SET raw_json = json_remove(raw_json, '$.dismissed'), seen = ?2 WHERE id = ?1", params![child, seen])?;
    if let Some(t) = topic {
        let state = if filed { "confirmed" } else { "suggested" };
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, ?3, 1.0, 'picked by you')
             ON CONFLICT (item_id, topic_id) DO UPDATE SET state = ?3, confidence = 1.0, reason = 'picked by you', updated_at = datetime('now')",
            params![child, t, state],
        )?;
    }
    settle_parts(conn, &[from, child])?;
    Ok(child)
}

/// Dismiss single messages in place: they join the family's dismissed part
/// (made on first use), which never shows in Incoming. Returns that part.
pub fn review_dismiss(conn: &Connection, from: i64, ids: &[i64]) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let (_, family) = super::split::family(&tx, from)?;
    let mut bin = None;
    for id in family {
        if is_dismissed_part(&tx, id)? {
            bin = Some(id);
            break;
        }
    }
    let bin = match bin {
        Some(b) if b != from => {
            super::split::move_messages(&tx, from, ids, b)?;
            b
        }
        Some(b) => b,
        None => {
            let child = super::split::carve(&tx, from, ids, true)?;
            tx.execute("UPDATE items SET raw_json = json_set(raw_json, '$.dismissed', json('true')) WHERE id = ?1", [child])?;
            tx.execute("UPDATE topic_links SET state = 'wrong' WHERE item_id = ?1 AND state != 'wrong'", [child])?;
            child
        }
    };
    settle_parts(&tx, &[bin, from])?;
    tx.commit()?;
    Ok(bin)
}

/// Confirm every part in the review that has a topic. Unsorted stays in Incoming.
pub fn review_confirm(conn: &Connection, item_id: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let (_, parts) = review_parts(&tx, item_id, None)?;
    let mut n = 0;
    for p in parts.iter().filter(|p| !p.dismissed) {
        if let Some(t) = &p.topic {
            confirm(&tx, p.item_id, t.id)?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// Reclassifying a filed signal (18c chronology, 18d search).

/// One item a reclassify touched and its links' states before (None: no link),
/// so `unreclassify` can put them back exactly.
#[derive(Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Moved {
    pub item_id: i64,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub seen: bool,
}

/// The items moving together: a recurring event's whole series, else the item.
fn series_of(conn: &Connection, item_id: i64) -> Result<Vec<i64>> {
    let mut ids: Vec<i64> = conn
        .prepare(
            "SELECT o.id FROM items i JOIN items o ON o.source_id = i.source_id
               AND json_extract(o.raw_json, '$.recurringEventId') = json_extract(i.raw_json, '$.recurringEventId')
             WHERE i.id = ?1",
        )?
        .query_map([item_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if !ids.contains(&item_id) {
        ids.push(item_id);
    }
    ids.sort_unstable();
    Ok(ids)
}

fn category_of(conn: &Connection, topic: i64) -> Result<String> {
    Ok(conn.query_row("SELECT category FROM topics WHERE id = ?1", [topic], |r| r.get(0))?)
}

/// A Gmail thread filed under a topic of another category: swap its category
/// label (`config.category_labels`), through the outbox.
pub fn relabel_gmail(conn: &Connection, item_id: i64, from_category: Option<&str>, to_category: &str) -> Result<()> {
    if from_category == Some(to_category) {
        return Ok(());
    }
    let row: Option<(i64, String, String, String)> = conn
        .query_row(
            "SELECT s.id, s.kind, s.config_json, i.raw_json FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((source_id, kind, config, raw)) = row else { return Ok(()) };
    if kind != "gmail" || crate::outbox::read_only(conn, source_id)?.is_some() {
        return Ok(());
    }
    let raw: Value = serde_json::from_str(&raw).unwrap_or_default();
    let Some(thread) = raw.get("thread_id").and_then(Value::as_str) else { return Ok(()) };
    let config: Value = serde_json::from_str(&config).unwrap_or_default();
    let label = |c: &str| config.pointer(&format!("/category_labels/{c}")).and_then(Value::as_str).map(str::to_owned);
    let (remove, add) = (from_category.and_then(label), label(to_category));
    if remove.is_none() && add.is_none() {
        return Ok(());
    }
    crate::outbox::enqueue(conn, source_id, "gmail.relabel", &json!({ "thread_id": thread, "remove": remove, "add": add }), Some(item_id))?;
    Ok(())
}

fn set_link(conn: &Connection, item_id: i64, topic_id: i64, state: Option<&str>) -> Result<()> {
    match state {
        None => conn.execute("DELETE FROM topic_links WHERE item_id = ?1 AND topic_id = ?2", params![item_id, topic_id])?,
        Some(st) => conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, ?3, 1.0, 'picked by you')
             ON CONFLICT (item_id, topic_id) DO UPDATE SET state = ?3, updated_at = datetime('now')",
            params![item_id, topic_id, st],
        )?,
    };
    Ok(())
}

/// Move a signal to another topic (`t` in the chronology or search). One
/// topic per signal: the old link becomes `wrong`, the new one `confirmed`,
/// filed straight away. Nothing is written to the source, except a Gmail
/// thread's category label when the category changes. Returns what to undo.
pub fn reclassify(conn: &Connection, item_id: i64, from: Option<i64>, to: i64) -> Result<Vec<Moved>> {
    if from == Some(to) {
        return Ok(vec![]);
    }
    let tx = conn.unchecked_transaction()?;
    let from_cat = from.map(|f| category_of(&tx, f)).transpose()?;
    let to_cat = category_of(&tx, to)?;
    let mut moved = vec![];
    for id in series_of(&tx, item_id)? {
        let seen: bool = tx.query_row("SELECT seen FROM items WHERE id = ?1", [id], |r| r.get(0))?;
        let m = Moved { item_id: id, from_state: from.map(|f| link_state(&tx, id, f)).transpose()?.flatten(), to_state: link_state(&tx, id, to)?, seen };
        if let (Some(f), Some(_)) = (from, &m.from_state) {
            set_link(&tx, id, f, Some("wrong"))?;
        }
        set_link(&tx, id, to, Some("confirmed"))?;
        tx.execute("UPDATE items SET seen = 1 WHERE id = ?1", [id])?;
        moved.push(m);
    }
    // Still in Incoming (from search): that's filing, so a Gmail thread is
    // labelled, marked read and archived as Confirm would. Filed already: only
    // its category label changes.
    let was_new = moved.iter().any(|m| m.item_id == item_id && !m.seen);
    match (was_new, settled_family(&tx, item_id)?) {
        (true, Some((source_id, kind, raws))) if kind == "gmail" => file_gmail(&tx, source_id, item_id, &raws, to)?,
        _ => relabel_gmail(&tx, item_id, from_cat.as_deref(), &to_cat)?,
    }
    settle_todo(&tx, item_id, to)?;
    tx.commit()?;
    Ok(moved)
}

/// `u` after a reclassify: every link back as it was.
pub fn unreclassify(conn: &Connection, moved: &[Moved], from: Option<i64>, to: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for m in moved {
        set_link(&tx, m.item_id, to, m.to_state.as_deref())?;
        if let Some(f) = from {
            set_link(&tx, m.item_id, f, m.from_state.as_deref())?;
        }
        tx.execute("UPDATE items SET seen = ?2 WHERE id = ?1", params![m.item_id, m.seen])?;
    }
    if let Some(first) = moved.first() {
        let from_cat = from.map(|f| category_of(&tx, f)).transpose()?;
        if let Some(fc) = from_cat {
            relabel_gmail(&tx, first.item_id, Some(&category_of(&tx, to)?), &fc)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// An entry expanded in place (18c, 18d): its messages with who wrote them,
/// the one context message before them, and whether it can be split.
#[derive(Serialize, Debug)]
pub struct Detail {
    pub item_id: i64,
    pub source: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub msg_count: Option<i64>,
    pub messages: Vec<Message>,
    /// The last context message before the signal, faded.
    pub context: Option<Message>,
}

pub fn detail(conn: &Connection, item_id: i64, settings: &PeopleSettings) -> Result<Detail> {
    let (source, url, summary, msg_count): (String, Option<String>, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT s.kind, i.url, i.summary, i.msg_count FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1 AND s.tag = 'work'",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| msg("no such item"))?;
    let look = Lookup { people: Resolver::load(conn, settings)?, suggest: HashMap::new() };
    let (own, mut context) = nest(message_rows(conn, &look, &source, item_id)?);
    Ok(Detail { item_id, source, url, summary, msg_count, messages: own.into_iter().map(|(m, _)| m).collect(), context: context.pop() })
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
        ("09:31", "Marco R.", "Backoff patch is up, needs a second reviewer."),
        ("10:02", "Sam", "Acme is asking for an exec on a call today."),
        ("10:40", "Jordan", "Fix ETA 14:00. Backlog is draining."),
    ] {
        ctx.slack(inc, &ctx.at(0, t), who, text);
    }
    seed_splits(ctx)?;
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
        ctx.slack(dm, &ctx.at(0, t), "Priya", text);
    }
    // Marco's third signal this week, so Incoming suggests a People note for him (17a).
    let eng = new(ItemSeed {
        source: "slack",
        kind: "message",
        title: "#platform-eng".into(),
        summary: Some("Anyone seen the staging deploy hang on migrations?".into()),
        url: Some("slack://channel?team=T0&id=C0PLAT".into()),
        occurred_at: ctx.at(0, "08:12"),
        msg_count: Some(1),
        ..Default::default()
    });
    ctx.slack(eng, &ctx.at(0, "08:12"), "Marco R.", "Anyone seen the staging deploy hang on migrations?");
    // His Slack profile (users.info), so the strip reads as in 17a.
    ctx.conn.execute(
        "INSERT INTO account_profiles (kind, account, real_name, email) VALUES ('slack', ?1, 'Marco Rossi', 'marco.rossi@work.com')",
        [crate::seed::slack_id("Marco R.")],
    )?;
    Ok(())
}

/// Two Slack signals the AI split across topics (15g: a DM; 15h: a channel
/// with a thread). The root keeps the Acme outage part; the others are split
/// children, one suggested to Q4 headcount and one left in Unsorted.
fn seed_splits(ctx: &Ctx) -> anyhow::Result<()> {
    #[derive(Clone, Copy, PartialEq)]
    enum G {
        Acme,
        Q4,
        Unsorted,
    }
    /// (time, sender, text, group, thread replies as (time, sender, text)).
    type Msg<'a> = (&'a str, &'a str, &'a str, G, &'a [(&'a str, &'a str, &'a str)]);
    let family = |title: &str, channel: &str, msgs: &[Msg]| -> anyhow::Result<()> {
        let first = msgs.first().map(|m| m.0).unwrap_or("09:00");
        let last = msgs.last().map(|m| m.0).unwrap_or(first);
        let keys: Vec<String> = msgs.iter().enumerate().map(|(i, _)| format!("{channel}.{i}")).collect();
        let root = ctx.item(ItemSeed {
            source: "slack",
            kind: "thread",
            title: title.into(),
            url: Some(format!("slack://channel?team=T0&id={channel}")),
            occurred_at: ctx.at(0, first),
            ended_at: Some(ctx.at(0, last)),
            msg_count: Some(msgs.len() as i64),
            seen: false,
            ..Default::default()
        });
        ctx.conn.execute(
            "UPDATE items SET raw_json = ?2 WHERE id = ?1",
            params![root, json!({ "channel": channel, "msgs": keys, "mark_ts": keys.last() }).to_string()],
        )?;
        let mut groups: Vec<(G, Vec<i64>)> = vec![];
        for ((t, who, text, g, replies), key) in msgs.iter().zip(&keys) {
            let id = ctx.slack_keyed(root, &ctx.at(0, t), who, text, Some(key), None);
            for (i, (rt, rwho, rtext)) in replies.iter().enumerate() {
                ctx.slack_keyed(root, &ctx.at(0, rt), rwho, rtext, Some(&format!("{key}.r{i}")), Some(key));
            }
            match groups.iter_mut().find(|x| x.0 == *g) {
                Some(x) => x.1.push(id),
                None => groups.push((*g, vec![id])),
            }
        }
        super::split::recount(ctx.conn, root, 0)?;
        for (g, ids) in groups {
            let item = if g == G::Acme { root } else { super::split::split_off(ctx.conn, root, &ids)? };
            match g {
                G::Acme => ctx.link(item, "Acme outage", "suggested"),
                G::Q4 => ctx.link(item, "Q4 headcount", "suggested"),
                G::Unsorted => {}
            }
        }
        Ok(())
    };
    use G::*;
    family(
        "DM · Jordan",
        "D0JORDAN",
        &[
            ("09:12", "Jordan", "Retry storm is contained, backlog is down to 3k.", Acme, &[]),
            ("09:13", "Zach", "Nice. Who's writing the RCA?", Acme, &[]),
            ("09:14", "Jordan", "Also, can we talk about the SRE req before Wed?", Q4, &[]),
            ("09:15", "Jordan", "Priya said it might move to Q1.", Q4, &[]),
            ("09:16", "Zach", "Yes. Tue 15:00?", Q4, &[]),
            ("09:40", "Jordan", "Acme wants the RCA by Friday. I can draft it.", Acme, &[]),
            ("09:41", "Jordan", "Who should review, you or Marco?", Acme, &[]),
            ("10:02", "Jordan", "lunch?", Unsorted, &[]),
            ("10:03", "Zach", "can't today, tomorrow?", Unsorted, &[]),
            ("10:04", "Jordan", "Also the Acme dashboards are still red.", Unsorted, &[]),
            ("10:30", "Jordan", "Fix is deployed, watching error rates.", Acme, &[]),
        ],
    )?;
    family(
        "#acme-incident",
        "C0ACME",
        &[
            (
                "09:05",
                "Sam",
                "Error rate on ingest is back above 2%. Anyone on it?",
                Acme,
                &[
                    ("09:07", "Jordan", "Looking. Same retry pattern as yesterday."),
                    ("09:09", "Zach", "Page Marco if it passes 5%."),
                    ("09:20", "Jordan", "Contained, backlog down to 3k."),
                    ("09:22", "Sam", "Thanks. I'll tell Acme."),
                ],
            ),
            ("09:30", "Priya", "Unrelated, but is the SRE req still on for Q4?", Q4, &[]),
            ("09:31", "Zach", "Yes, let's take it to our 1:1.", Q4, &[]),
            ("09:48", "Marco R.", "anyone else's VPN flaky today?", Unsorted, &[]),
            ("10:15", "Jordan", "Fix is deployed, watching error rates.", Acme, &[]),
            ("10:40", "Sam", "🎉 status page is green", Unsorted, &[]),
        ],
    )?;
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
    fn events_say_when_and_what_changed() {
        let now = parse_ts("2026-09-28T10:00:00").unwrap();
        assert_eq!(event_meta("2026-10-01T17:00:00", Some("2026-10-01T19:00:00"), Some("Sam"), now), "Sam · Thu 17:00–19:00");
        assert_eq!(event_meta("2026-10-01T00:00:00", Some("2026-10-03T00:00:00"), None, now), "Thu–Fri");
        assert_eq!(event_meta("2026-09-28T00:00:00", Some("2026-09-29T00:00:00"), None, now), "Today");
        assert_eq!(change_label("moved", Some("2026-09-29T14:00:00"), now).as_deref(), Some("Moved from Tue 14:00"));
        assert_eq!(change_label("cancelled", None, now).as_deref(), Some("Cancelled"));
        assert_eq!(excerpt("abcdef", 3), "abc…");
    }

    #[test]
    fn seed_matches_mock() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let names: Vec<_> = q.topics.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Acme outage", "Alerting · P2", "Q4 headcount", "v1 ingest deprecation", "Career"]);
        let acme = &q.topics[0];
        assert_eq!(acme.signals.len(), 5, "inc-2291, the two split Slack signals (15g, 15h), mail, invite");
        let slack = acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap();
        assert_eq!(slack.meta, "42 new · 09:05–10:40");
        assert_eq!(slack.messages.len(), 4);
        let cal = acme.signals.iter().find(|s| s.source == "gcal").unwrap();
        assert_eq!(cal.meta, "Sam · Today 10:41");
        let mail = acme.signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail.meta, "10:22");
        let alerts: i64 = q.topics[1].signals.iter().filter_map(|s| s.msg_count).sum();
        assert_eq!(alerts, 214);
        assert_eq!(q.unsorted.len(), 9, "6 items, the Unsorted parts of both splits, #platform-eng");
        assert!(q.unsorted.iter().any(|s| s.source == "github"));
        assert!(q.unsorted.iter().any(|s| s.source == "jira"));
        // The tab badge (unseen items) matches what the queue shows.
        assert_eq!(unseen(&conn) as usize, shown(&q));
    }

    #[test]
    fn a_triaged_channel_run_drops_the_channel_context() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let s = q.topics.iter().flat_map(|t| &t.signals).chain(&q.unsorted).find(|s| s.source == "slack" && !s.split && !s.messages.is_empty()).unwrap();
        let (id, first) = (s.item_id, s.messages[0].id);
        conn.execute("INSERT INTO item_messages (item_id, at, author, text, context) VALUES (?1, '2000-01-01T00:00:00', 'x', 'earlier in the channel', 1)", [id]).unwrap();
        conn.execute("UPDATE items SET raw_json = json_remove(raw_json, '$.thread_ts') WHERE id = ?1", [id]).unwrap();
        let find = |q: &Queue| q.topics.iter().flat_map(|t| &t.signals).chain(&q.unsorted).find(|x| x.item_id == id).map(|x| x.context.len()).unwrap();
        assert!(find(&queue(&conn, now).unwrap()) > 0, "nothing read yet: context shows");
        conn.execute("UPDATE item_messages SET read = 1 WHERE id = ?1", [first]).unwrap();
        assert_eq!(find(&queue(&conn, now).unwrap()), 0, "already read: no channel context");
        conn.execute("UPDATE items SET raw_json = json_set(raw_json, '$.thread_ts', '1.0') WHERE id = ?1", [id]).unwrap();
        assert!(find(&queue(&conn, now).unwrap()) > 0, "a thread keeps its parent as context");
    }

    #[test]
    fn gmail_signal_carries_its_gmail_category() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let mail = q.topics[0].signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail.category, None, "no category stored yet");
        conn.execute("UPDATE items SET raw_json = json_set(raw_json, '$.category', 'vendors') WHERE id = ?1", [mail.item_id]).unwrap();
        let q2 = queue(&conn, now).unwrap();
        let mail2 = q2.topics[0].signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail2.category.as_deref(), Some("vendors"));
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
        assert_eq!(q2.topics[topic(&q2, "Acme outage").unwrap()].signals.len(), 4);
        assert!(!q2.unsorted.iter().any(|s| s.item_id == item));
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    #[test]
    fn updates_on_filed_items_show_under_their_topic() {
        let (conn, now, _d) = setup();
        let (item, topic): (i64, i64) = conn
            .query_row("SELECT item_id, topic_id FROM topic_links WHERE state = 'confirmed' LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [item]).unwrap();
        let q = queue(&conn, now).unwrap();
        let t = q.topics.iter().find(|t| t.id == topic).unwrap();
        assert!(t.signals.iter().any(|s| s.item_id == item && s.update));
        assert_eq!(unseen(&conn) as usize, shown(&q));
        confirm(&conn, item, topic).unwrap();
        assert_eq!(unseen(&conn) as usize, shown(&queue(&conn, now).unwrap()));
    }

    #[test]
    fn dismiss_leaves_triage_and_undo_brings_it_back() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let item = q.unsorted[0].item_id;
        dismiss(&conn, item, true).unwrap();
        let q2 = queue(&conn, now).unwrap();
        assert!(!q2.unsorted.iter().any(|s| s.item_id == item));
        assert_eq!(unseen(&conn) as usize, shown(&q2));
        dismiss(&conn, item, false).unwrap();
        assert!(queue(&conn, now).unwrap().unsorted.iter().any(|s| s.item_id == item));
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
        assert_eq!(q2.unsorted.len(), 8);
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    fn topic_named(conn: &Connection, name: &str) -> i64 {
        conn.query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    }

    fn exists(conn: &Connection, item: i64) -> bool {
        conn.query_row("SELECT EXISTS (SELECT 1 FROM items WHERE id = ?1)", [item], |r| r.get(0)).unwrap()
    }

    #[test]
    fn messages_carry_people_and_unknown_senders_get_a_suggestion() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let inc = acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap();
        let jordan = &inc.messages[0].who;
        assert!(jordan.known && jordan.name.as_deref() == Some("Jordan"));
        assert_eq!(jordan.color, people::hash_color("jordan.lee@work.com"));
        let marco = &inc.messages[1].who;
        assert!(!marco.known);
        assert_eq!(marco.account.as_ref().map(|a| a.id.as_str()), Some("U0MARCO"));
        assert_eq!(marco.color, people::hash_color("marco.rossi@work.com"), "coloured by his profile email");
        let np = inc.new_person.as_ref().expect("Marco is in 3 signals today");
        assert_eq!((np.account.as_str(), np.name.as_str(), np.signals), ("U0MARCO", "Marco R.", 3));
        assert_eq!((np.real_name.as_deref(), np.email.as_deref()), (Some("Marco Rossi"), Some("marco.rossi@work.com")));
        let invite = acme.signals.iter().find(|s| s.source == "gcal").unwrap();
        let author = invite.author.as_ref().expect("the invite's organiser");
        assert_eq!((author.name.as_str(), author.who.known), ("Sam", true), "matched like a message sender");
        assert!(invite.meta.starts_with("Sam · "));
        let dm = acme.signals.iter().find(|s| s.title == "DM · Jordan").unwrap();
        assert!(dm.messages[1].who.me, "your own messages are you, in your colour");
        assert_eq!(dm.messages[1].who.color, 0);
        assert!(dm.new_person.is_none());
    }

    #[test]
    fn a_split_signal_shows_where_its_other_parts_came_in() {
        let (conn, now, _d) = setup();
        let q = queue(&conn, now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let dm = acme.signals.iter().find(|s| s.title == "DM · Jordan").unwrap();
        assert!(dm.split);
        let times: Vec<(&str, bool)> = dm.messages.iter().map(|m| (m.time.as_str(), m.gap)).collect();
        assert_eq!(times, [("09:12", false), ("09:13", false), ("09:40", true), ("09:41", false), ("10:30", true)]);
        assert!(!acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap().split);
        let chan = acme.signals.iter().find(|s| s.title == "#acme-incident").unwrap();
        assert_eq!(chan.messages[0].replies.len(), 4, "the thread rides with its first message");
    }

    #[test]
    fn split_review_moves_dismisses_and_confirms_parts() {
        let (conn, now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let q4 = topic_named(&conn, "Q4 headcount");
        let dm: i64 = conn.query_row("SELECT id FROM items WHERE title = 'DM · Jordan' AND parent_id IS NULL", [], |r| r.get(0)).unwrap();
        let f = family(&conn, dm, None, &settings).unwrap();
        assert_eq!((f.conv, f.root, f.messages.len()), ("dm", dm, 11));
        let topics: Vec<Option<i64>> = f.parts.iter().map(|p| p.topic.as_ref().map(|t| t.id)).collect();
        assert_eq!(topics, [Some(acme), Some(q4), None]);
        let uns = f.parts[2].item_id;
        let at = |t: &str| f.messages.iter().find(|m| m.time == t).unwrap().id;
        assert!(f.messages.iter().all(|m| m.part.is_some()));

        // t to a topic that has a group: straight into that part; u moves it back.
        assert_eq!(review_move(&conn, uns, &[at("10:04")], To::Topic(acme), None).unwrap(), dm);
        assert_eq!(family(&conn, dm, None, &settings).unwrap().messages.iter().find(|m| m.time == "10:04").unwrap().part, Some(dm));
        review_move(&conn, dm, &[at("10:04")], To::Part(uns), None).unwrap();

        // A topic with no group yet: a new part, suggested to it.
        let career = topic_named(&conn, "Career");
        let part = review_move(&conn, uns, &[at("10:02")], To::Topic(career), None).unwrap();
        assert!(part != uns && part != dm);
        assert_eq!(link_state(&conn, part, career).unwrap().as_deref(), Some("suggested"));
        // d: dismissed in place, into the family's seen part; a second one joins it.
        let bin = review_dismiss(&conn, uns, &[at("10:03")]).unwrap();
        assert_eq!(review_dismiss(&conn, uns, &[at("10:04")]).unwrap(), bin);
        let f2 = family(&conn, dm, None, &settings).unwrap();
        assert!(f2.parts.iter().any(|p| p.item_id == bin && p.dismissed));
        assert_eq!(f2.messages.len(), 11, "dismissed messages stay in the review, struck through");
        assert!(!exists(&conn, uns), "the emptied Unsorted part is pruned");
        // u: back to Unsorted, which is made again.
        let uns2 = review_move(&conn, bin, &[at("10:04")], To::Unsorted, None).unwrap();
        assert_eq!(review_move(&conn, bin, &[at("10:03")], To::Unsorted, None).unwrap(), uns2);
        assert!(!exists(&conn, bin), "the dismissed part goes once it's empty");
        let seen: bool = conn.query_row("SELECT seen FROM items WHERE id = ?1", [uns2], |r| r.get(0)).unwrap();
        assert!(!seen);
        // Moving every message out of a part removes it.
        let career_msgs = super::super::split::message_ids(&conn, part).unwrap();
        review_move(&conn, part, &career_msgs, To::Unsorted, None).unwrap();
        assert!(!exists(&conn, part));
        assert_eq!(super::super::split::message_ids(&conn, uns2).unwrap().len(), 3);

        // ↵ confirms every part with a topic; Unsorted stays.
        assert_eq!(review_confirm(&conn, dm).unwrap(), 2);
        assert_eq!(link_state(&conn, dm, acme).unwrap().as_deref(), Some("confirmed"));
        let q = queue(&conn, now).unwrap();
        assert!(!q.topics.iter().flat_map(|t| &t.signals).any(|s| s.title == "DM · Jordan"));
        assert!(q.unsorted.iter().any(|s| s.item_id == uns2));
        assert_eq!(unseen(&conn) as usize, shown(&q));
        // Moving Unsorted's messages into a confirmed part files them there:
        // the family is settled and the conversation is marked read once.
        let rest = super::super::split::message_ids(&conn, uns2).unwrap();
        review_move(&conn, uns2, &rest, To::Part(dm), None).unwrap();
        let marks: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| r.get(0)).unwrap();
        assert_eq!(marks, 0, "a move isn't a confirm");
    }

    #[test]
    fn reclassify_moves_a_filed_signal_and_undo_puts_every_link_back() {
        let (conn, _now, _d) = setup();
        let acme = topic_named(&conn, "Acme outage");
        let webhook = topic_named(&conn, "Webhook reliability");
        let item: i64 = conn
            .query_row("SELECT item_id FROM topic_links WHERE topic_id = ?1 AND state = 'confirmed' LIMIT 1", [acme], |r| r.get(0))
            .unwrap();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'wrong')", params![item, webhook]).unwrap();
        let moved = reclassify(&conn, item, Some(acme), webhook).unwrap();
        assert_eq!(moved, [Moved { item_id: item, from_state: Some("confirmed".into()), to_state: Some("wrong".into()), seen: true }]);
        assert_eq!(link_state(&conn, item, acme).unwrap().as_deref(), Some("wrong"), "one topic per signal");
        assert_eq!(link_state(&conn, item, webhook).unwrap().as_deref(), Some("confirmed"), "filed straight away");
        unreclassify(&conn, &moved, Some(acme), webhook).unwrap();
        assert_eq!(link_state(&conn, item, acme).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, item, webhook).unwrap().as_deref(), Some("wrong"));
        // A signal with no topic (from search) just gets one; undo removes it.
        let pr: i64 = conn.query_row("SELECT id FROM items WHERE kind = 'pr' AND seen = 0 LIMIT 1", [], |r| r.get(0)).unwrap();
        let moved = reclassify(&conn, pr, None, webhook).unwrap();
        assert_eq!(link_state(&conn, pr, webhook).unwrap().as_deref(), Some("confirmed"));
        unreclassify(&conn, &moved, None, webhook).unwrap();
        assert_eq!(link_state(&conn, pr, webhook).unwrap(), None);
        let seen: bool = conn.query_row("SELECT seen FROM items WHERE id = ?1", [pr], |r| r.get(0)).unwrap();
        assert!(!seen, "back in Incoming as it was");
    }

    #[test]
    fn reclassifying_a_gmail_thread_swaps_its_category_label() {
        let (conn, _now, _d) = setup();
        conn.execute("UPDATE sources SET config_json = '{\"category_labels\": {\"work\": \"Work\", \"jobs\": \"Jobs\"}}' WHERE kind = 'gmail'", []).unwrap();
        let career = topic_named(&conn, "Career");
        let acme = topic_named(&conn, "Acme outage");
        let mail: i64 = conn.query_row("SELECT id FROM items WHERE title LIKE 'Lee%calibration%'", [], |r| r.get(0)).unwrap();
        conn.execute("UPDATE items SET raw_json = '{\"thread_id\": \"t1\"}' WHERE id = ?1", [mail]).unwrap();
        confirm(&conn, mail, career).unwrap();
        conn.execute("DELETE FROM outbox", []).unwrap();
        reclassify(&conn, mail, Some(career), acme).unwrap();
        let payload: String = conn.query_row("SELECT payload_json FROM outbox WHERE op = 'gmail.relabel'", [], |r| r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), json!({ "thread_id": "t1", "remove": "Jobs", "add": "Work" }));
    }

    #[test]
    fn a_filed_signals_review_files_moves_straight_away() {
        let (conn, _now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let webhook = topic_named(&conn, "Webhook reliability");
        let dm: i64 = conn.query_row("SELECT id FROM items WHERE title = 'DM · Jordan' AND parent_id IS NULL", [], |r| r.get(0)).unwrap();
        review_confirm(&conn, dm).unwrap();
        // Reviewing just this filed signal: only its own part (and nothing from Incoming).
        let f = family(&conn, dm, Some(&[dm]), &settings).unwrap();
        assert_eq!(f.parts.len(), 1);
        assert_eq!(f.parts[0].topic.as_ref().map(|t| t.id), Some(acme));
        assert_eq!(f.messages.len(), 5);
        let last = f.messages.last().unwrap().id;
        let part = review_move(&conn, dm, &[last], To::Topic(webhook), Some(&[dm])).unwrap();
        assert_eq!(link_state(&conn, part, webhook).unwrap().as_deref(), Some("confirmed"));
        let seen: bool = conn.query_row("SELECT seen FROM items WHERE id = ?1", [part], |r| r.get(0)).unwrap();
        assert!(seen, "filed, not back in Incoming");
        let f = family(&conn, dm, Some(&[dm, part]), &settings).unwrap();
        assert_eq!(f.parts.len(), 2);
        // u: back to its old group by topic.
        assert_eq!(review_move(&conn, part, &[last], To::Topic(acme), Some(&[dm, part])).unwrap(), dm);
        assert!(!exists(&conn, part));
        let d = detail(&conn, dm, &settings).unwrap();
        assert_eq!((d.messages.len(), d.source.as_str()), (5, "slack"));
    }

    #[test]
    fn an_unsplit_signal_can_be_split_in_the_review() {
        let (conn, now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let q4 = topic_named(&conn, "Q4 headcount");
        // Under a topic: one part, its suggested topic.
        let inc: i64 = conn.query_row("SELECT id FROM items WHERE title = '#inc-2291'", [], |r| r.get(0)).unwrap();
        let f = family(&conn, inc, None, &settings).unwrap();
        assert_eq!(f.parts.len(), 1);
        assert_eq!(f.parts[0].topic.as_ref().map(|t| t.id), Some(acme));
        let marco = f.messages.iter().find(|m| m.author == "Marco R.").unwrap().id;
        let part = review_move(&conn, inc, &[marco], To::Topic(q4), None).unwrap();
        assert_ne!(part, inc);
        assert_eq!(link_state(&conn, part, q4).unwrap().as_deref(), Some("suggested"));
        assert_eq!(review_confirm(&conn, inc).unwrap(), 2);
        assert_eq!(link_state(&conn, inc, acme).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, part, q4).unwrap().as_deref(), Some("confirmed"));

        // In Unsorted: the rest stays there.
        let dm: i64 = conn.query_row("SELECT id FROM items WHERE title = 'DM · Priya'", [], |r| r.get(0)).unwrap();
        let f = family(&conn, dm, None, &settings).unwrap();
        assert!(f.parts[0].topic.is_none());
        let first = f.messages[0].id;
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        review_move(&conn, dm, &[first], To::Topic(v1), None).unwrap();
        assert_eq!(review_confirm(&conn, dm).unwrap(), 1);
        let q = queue(&conn, now).unwrap();
        assert!(q.unsorted.iter().any(|s| s.item_id == dm && s.messages.len() == 3));
        assert_eq!(unseen(&conn) as usize, shown(&q));
    }

    #[test]
    fn confirming_a_review_leaves_the_unsorted_group_in_incoming() {
        let (conn, now, _d) = setup();
        let chan: i64 = conn.query_row("SELECT id FROM items WHERE title = '#acme-incident' AND parent_id IS NULL", [], |r| r.get(0)).unwrap();
        let f = family(&conn, chan, None, &PeopleSettings::default()).unwrap();
        assert_eq!(f.conv, "channel");
        assert_eq!(f.messages[0].replies.len(), 4);
        let uns = f.parts.iter().find(|p| p.topic.is_none()).unwrap().item_id;
        assert_eq!(review_confirm(&conn, chan).unwrap(), 2);
        let q = queue(&conn, now).unwrap();
        assert!(q.unsorted.iter().any(|s| s.item_id == uns));
        let marks: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| r.get(0)).unwrap();
        assert_eq!(marks, 0, "not marked read while part of it is still in Incoming");
        assert!(review_move(&conn, uns, &[9999], To::Part(chan), None).is_err());
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

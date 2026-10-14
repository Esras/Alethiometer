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
use super::{arg_i64, opt_i64, when_label};
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::incoming::{MessageRow, SignalRow};
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::outbox::request;
use crate::seed::{Ctx, ItemSeed};
use crate::settings::sources::is_chat;
use crate::settings::PeopleSettings;
use crate::time::Ts;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize, Debug)]
pub struct Message {
    /// `item_messages.id`: what a split names.
    pub id: i64,
    /// `item_messages.ext_key` (a Slack `ts`): what "open" links to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
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
    /// Stored context messages, oldest first. Empty in `list`, which leaves
    /// them to `messages` for the rows you expand (agent signals excepted).
    pub messages: Vec<Message>,
    pub context: Vec<Message>,
    /// How many top-level messages `messages` holds (or would).
    pub n_messages: usize,
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
    /// An agent signal (`agents::incoming`): who, why, the session, its request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Value>,
    /// Agents waiting: the topic it's filed under (none: Unsorted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<i64>,
    /// Last activity: a thread counts from its latest message.
    #[serde(skip)]
    at: Ts,
    #[serde(skip)]
    author_handle: Option<String>,
    #[serde(skip)]
    author_name: Option<String>,
    /// A Slack thread item: a reply goes in its thread by default.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    thread: bool,
    /// An invite's Accept/Decline line (14a).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rsvp: Option<super::respond::Rsvp>,
    /// The latest Slack reply sent from this signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<super::respond::Answer>,
    /// The item's revision as listed. Triage sends it back with `keys`.
    pub rev: i64,
    /// The split family it belongs to (its first part's item).
    pub root: i64,
    /// Keys of the messages listed (`item_messages.ext_key`).
    pub keys: Vec<String>,
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
    /// Agents waiting on you, across topics: the same signals again (2b).
    pub agents: Vec<Signal>,
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "list" => Route::Read(|r, ctx, _| Ok(json!(list(r, ctx.clock, &ctx.settings.people)?))),
        "family" => Route::Read(|r, ctx, args| {
            Ok(json!(family(
                r,
                ctx.clock,
                arg_i64(args, "item_id")?,
                scope_arg(args).as_deref(),
                &ctx.settings.people
            )?))
        }),
        "detail" => Route::Read(|r, ctx, args| {
            Ok(json!(detail(
                r,
                ctx.clock,
                arg_i64(args, "item_id")?,
                &ctx.settings.people
            )?))
        }),
        "messages" => {
            Route::Read(|r, ctx, args| messages(r, ctx.clock, arg_i64(args, "item_id")?, &ctx.settings.people))
        }
        "reclassify" => Route::Write(|uow, ctx, args| {
            let moved = reclassify(
                uow,
                ctx.clock.now(),
                arg_i64(args, "item_id")?,
                opt_i64(args, "from_topic_id"),
                arg_i64(args, "to_topic_id")?,
            )?;
            Ok(json!({ "ok": true, "moved": moved }))
        }),
        "unreclassify" => Route::Write(|uow, _, args| {
            let moved: Vec<Moved> = serde_json::from_value(args.get("moved").cloned().unwrap_or_default())
                .map_err(|e| msg(format!("bad 'moved': {e}")))?;
            unreclassify(
                uow,
                &moved,
                opt_i64(args, "from_topic_id"),
                arg_i64(args, "to_topic_id")?,
            )?;
            Ok(json!({ "ok": true }))
        }),
        "review_move" => Route::Write(|uow, _, args| {
            let to = match (opt_i64(args, "to_item_id"), opt_i64(args, "topic_id")) {
                (Some(part), _) => To::Part(part),
                (None, Some(topic)) => To::Topic(topic),
                // No topic: Unsorted.
                (None, None) => To::Unsorted,
            };
            let part = review_move(
                uow,
                arg_i64(args, "item_id")?,
                &ids_arg(args),
                to,
                scope_arg(args).as_deref(),
            )?;
            Ok(json!({ "ok": true, "item_id": part }))
        }),
        "review_dismiss" => Route::Write(|uow, _, args| {
            let part = review_dismiss(uow, arg_i64(args, "item_id")?, &ids_arg(args))?;
            Ok(json!({ "ok": true, "item_id": part }))
        }),
        "review_confirm" => Route::Write(|uow, ctx, args| {
            Ok(json!({ "ok": true, "confirmed": review_confirm(uow, ctx.clock.now(), arg_i64(args, "item_id")?)? }))
        }),
        "confirm" => Route::Write(|uow, ctx, args| {
            let (item, topic) = (arg_i64(args, "item_id")?, arg_i64(args, "topic_id")?);
            let moved = snapshot(uow, item, None, topic)?;
            let changed = confirm_seen(uow, ctx.clock.now(), item, topic, saw_arg(args).as_ref())?;
            Ok(json!({ "ok": true, "changed": changed, "moved": moved }))
        }),
        "wrong" => Route::Write(|uow, _, args| {
            let (item, topic) = (arg_i64(args, "item_id")?, arg_i64(args, "topic_id")?);
            let moved = snapshot(uow, item, None, topic)?;
            wrong(uow, item, topic)?;
            Ok(json!({ "ok": true, "moved": moved }))
        }),
        "move" => Route::Write(|uow, ctx, args| {
            let (item, topic, from) = (
                arg_i64(args, "item_id")?,
                arg_i64(args, "topic_id")?,
                opt_i64(args, "from_topic_id"),
            );
            let moved = snapshot(uow, item, from, topic)?;
            let changed = move_seen(uow, ctx.clock.now(), item, topic, from, saw_arg(args).as_ref())?;
            Ok(json!({ "ok": true, "changed": changed, "moved": moved }))
        }),
        "unfile" => Route::Write(|uow, _, args| {
            let moved: Vec<Moved> = serde_json::from_value(args.get("moved").cloned().unwrap_or_default())
                .map_err(|e| msg(format!("bad 'moved': {e}")))?;
            unfile(
                uow,
                &moved,
                opt_i64(args, "from_topic_id"),
                arg_i64(args, "to_topic_id")?,
                args.get("filed").and_then(Value::as_bool).unwrap_or(false),
            )?;
            Ok(json!({ "ok": true }))
        }),
        "dismiss" => Route::Write(|uow, _, args| {
            let item = arg_i64(args, "item_id")?;
            let unread = uow.items().unread_keys(item)?;
            let changed = dismiss_seen(uow, item, saw_arg(args).as_ref())?;
            Ok(json!({ "ok": true, "changed": changed, "unread": unread }))
        }),
        "undismiss" => Route::Write(|uow, _, args| {
            let item = arg_i64(args, "item_id")?;
            dismiss(uow, item, false)?;
            let unread: Vec<String> = args
                .get("unread")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            uow.items().mark_unread(item, &unread)?;
            Ok(json!({ "ok": true }))
        }),
        "trash" => Route::Write(|uow, _, args| {
            let item = arg_i64(args, "item_id")?;
            let unread = uow.items().unread_keys(item)?;
            let changed = trash_seen(uow, item, saw_arg(args).as_ref())?;
            Ok(json!({ "ok": true, "changed": changed, "unread": unread }))
        }),
        "split" => Route::Write(|uow, ctx, args| {
            let child = split_to(
                uow,
                ctx.clock.now(),
                arg_i64(args, "item_id")?,
                &ids_arg(args),
                arg_i64(args, "topic_id")?,
            )?;
            Ok(json!({ "ok": true, "item_id": child }))
        }),
        _ => return None,
    })
}

/// A split review of filed signals (18c/18d) names the parts it works on; Incoming's doesn't.
fn scope_arg(args: &Value) -> Option<Vec<i64>> {
    args.get("parts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
}

/// What the list showed of a signal when you acted on it: its revision and
/// the keys of its messages (`Signal::rev`, `Signal::keys`). Triage applies
/// to that: those messages are read, Gmail and Slack act only up to them,
/// and if the item gained messages or came back since, it stays in Incoming
/// with only the newer ones new (`app/docs/db-plan.md`, Phase 3).
#[derive(Debug, Clone, Default)]
pub struct Saw {
    pub rev: i64,
    pub keys: Vec<String>,
}

fn saw_arg(args: &Value) -> Option<Saw> {
    let rev = args.get("rev").and_then(Value::as_i64)?;
    let keys = args
        .get("keys")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    Some(Saw { rev, keys })
}

/// After triage: the messages you saw are read; if the item changed since you
/// saw it, it stays in Incoming. Returns whether it had changed.
fn settle_saw(uow: &dyn UnitOfWork, item_id: i64, saw: Option<&Saw>) -> Result<bool> {
    let Some(saw) = saw else { return Ok(false) };
    uow.items().mark_read(item_id, &saw.keys)?;
    let changed = uow.items().rev(item_id)? != Some(saw.rev);
    if changed {
        uow.items().set_seen(item_id, false)?;
    }
    Ok(changed)
}

fn ids_arg(args: &Value) -> Vec<i64> {
    args.get("message_ids")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

fn to_signal(row: SignalRow, clock: &Clock) -> Signal {
    let agent: Option<Value> = row.agent_json.as_deref().and_then(|a| serde_json::from_str(a).ok());
    let time = meta(&row.title, row.occurred_at, row.ended_at, row.msg_count, None, clock);
    Signal {
        meta: match &agent {
            Some(a) => agent_meta(a, None, &time),
            None if row.kind == "event" => event_meta(row.occurred_at, row.ended_at, row.author_name.as_deref(), clock),
            None => meta(
                &row.title,
                row.occurred_at,
                row.ended_at,
                row.msg_count,
                row.author_name.as_deref(),
                clock,
            ),
        },
        change: row
            .change
            .as_deref()
            .and_then(|c| change_label(c, row.moved_from, clock)),
        description: row
            .description
            .map(|d| excerpt(d.trim(), 400))
            .filter(|d| !d.is_empty()),
        item_id: row.item_id,
        source: row.source_kind,
        kind: row.kind,
        title: row.title,
        summary: row.summary,
        url: row.url,
        msg_count: row.msg_count,
        messages: vec![],
        n_messages: 0,
        context: vec![],
        update: false,
        held: None,
        split: false,
        new_person: None,
        author: None,
        category: row.category,
        agent,
        topic: None,
        topic_id: None,
        at: row.ended_at.unwrap_or(row.occurred_at),
        author_handle: row.author_handle,
        author_name: row.author_name,
        thread: row.thread,
        rsvp: None,
        reply: None,
        rev: row.rev,
        root: row.root,
        keys: vec![],
    }
}

/// "HH:MM" today, otherwise "Thu 14:00" / "Sep 25" (here).
pub(crate) fn time_label(t: Ts, clock: &Clock) -> String {
    let (t, now) = (clock.local(t), clock.local_now());
    if t.date() == now.date() {
        t.strftime("%H:%M").to_string()
    } else {
        when_label(t, now, true)
    }
}

fn day_time(t: Ts, clock: &Clock) -> String {
    let t = clock.local(t);
    format!("{} {}", when_label(t, clock.local_now(), false), t.strftime("%H:%M"))
}

/// "Sam · Thu 11:00–11:45": when the event is, not when it was sent.
fn event_meta(start: Ts, ended: Option<Ts>, author: Option<&str>, clock: &Clock) -> String {
    let end = ended.filter(|e| *e > start);
    let now = clock.local_now();
    let (ls, le) = (clock.local(start), end.map(|e| clock.local(e)));
    // All-day events run midnight to midnight here.
    let midnight = |t: jiff::civil::DateTime| t.time() == jiff::civil::Time::midnight();
    let when = match le {
        Some(e) if midnight(ls) && midnight(e) => {
            let last = e.date().yesterday().expect("in range");
            if last > ls.date() {
                format!(
                    "{}–{}",
                    when_label(ls, now, false),
                    when_label(last.to_datetime(jiff::civil::Time::midnight()), now, false)
                )
            } else {
                when_label(ls, now, false)
            }
        }
        Some(e) if e.date() == ls.date() => format!("{}–{}", day_time(start, clock), e.strftime("%H:%M")),
        Some(_) => format!(
            "{}–{}",
            day_time(start, clock),
            day_time(end.expect("matched Some"), clock)
        ),
        None => day_time(start, clock),
    };
    match author {
        Some(a) => format!("{a} · {when}"),
        None => when,
    }
}

fn change_label(change: &str, moved_from: Option<Ts>, clock: &Clock) -> Option<String> {
    Some(match change {
        "invite" => "New invite".into(),
        "new" => "New event".into(),
        "moved" => match moved_from {
            Some(t) => format!("Moved from {}", day_time(t, clock)),
            None => "Moved".into(),
        },
        "guests" => "Guests changed".into(),
        "updated" => "Updated".into(),
        "cancelled" => "Cancelled".into(),
        _ => return None,
    })
}

/// An agent signal's meta: "ingest-svc › Webhooks · 11:22"; in Agents waiting,
/// "Webhooks · Jira webhooks · 11:22" (its group, then its topic).
fn agent_meta(a: &Value, topic: Option<Option<&str>>, time: &str) -> String {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let (project, group, cwd) = (s("project"), s("group"), s("cwd"));
    let place = match (project.is_empty(), group.is_empty()) {
        (false, false) if topic.is_none() => format!("{project} › {group}"),
        (_, false) => group,
        (false, true) => project,
        _ => cwd,
    };
    let mut parts = vec![place];
    if let Some(t) = topic {
        parts.push(t.unwrap_or("Unsorted").to_owned());
    }
    parts.push(time.to_owned());
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn excerpt(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

fn meta(title: &str, start: Ts, end: Option<Ts>, msgs: Option<i64>, author: Option<&str>, clock: &Clock) -> String {
    match (msgs, end) {
        (Some(n), Some(e)) if n > 1 => {
            let range = if clock.day_of(e) == clock.day_of(start) {
                format!("{}–{}", time_label(start, clock), clock.local(e).strftime("%H:%M"))
            } else {
                format!("{}–{}", time_label(start, clock), time_label(e, clock))
            };
            format!("{n} new · {range}")
        }
        _ => match author {
            // Mail and DM titles already name the sender ("Dana · …", "DM · Priya").
            Some(a) if !title.contains(a) => format!("{a} · {}", time_label(start, clock)),
            _ => time_label(start, clock),
        },
    }
}

/// What message rows need besides their own columns: who wrote them, and
/// which unknown senders are worth a People note.
pub struct Lookup {
    /// Message times are shown as wall-clock time here.
    pub clock: Clock,
    pub people: Resolver,
    pub suggest: HashMap<(String, String), Suggestion>,
}

impl Lookup {
    pub fn load(r: &dyn Reads, settings: &PeopleSettings, clock: &Clock) -> Result<Lookup> {
        let people = Resolver::load(r, settings)?;
        let suggest = people::suggestions(r, &people, settings, clock.now())?;
        Ok(Lookup {
            clock: clock.clone(),
            people,
            suggest,
        })
    }

    /// Without new-person suggestions (they cost a scan of recent messages).
    fn plain(r: &dyn Reads, settings: &PeopleSettings, clock: &Clock) -> Result<Lookup> {
        Ok(Lookup {
            clock: clock.clone(),
            people: Resolver::load(r, settings)?,
            suggest: HashMap::new(),
        })
    }
}

/// (message, context, ext_key, reply_to, author_id, at) for an item, oldest first.
type MsgRow = (Message, bool, Option<String>, Option<String>, Option<String>, Ts);

/// Messages of several items, by item.
fn messages_by_item(r: &dyn Reads, item_ids: &[i64]) -> Result<HashMap<i64, Vec<MessageRow>>> {
    Ok(by_item(r.incoming().messages(item_ids)?))
}

fn by_item(rows: Vec<MessageRow>) -> HashMap<i64, Vec<MessageRow>> {
    let mut by: HashMap<i64, Vec<MessageRow>> = HashMap::new();
    for m in rows {
        by.entry(m.item_id).or_default().push(m);
    }
    by
}

fn message_rows(look: &Lookup, source: &str, rows: Vec<MessageRow>) -> Vec<MsgRow> {
    rows.into_iter()
        .map(|m| {
            let msg = Message {
                id: m.id,
                key: m.ext_key.clone(),
                time: look.clock.local(m.at).strftime("%H:%M").to_string(),
                who: look.people.who(source, m.author_id.as_deref(), &m.author),
                author: m.author,
                text: m.text,
                gap: false,
                part: None,
                replies: vec![],
                read: m.read,
            };
            (msg, m.context, m.ext_key, m.reply_to, m.author_id, m.at)
        })
        .collect()
}

/// Top-level messages with their replies nested, and the context messages.
fn nest(rows: Vec<MsgRow>) -> (Vec<(Message, Ts)>, Vec<Message>) {
    let (replies, rows): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| r.3.is_some());
    let (context, mut own): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| r.1);
    for r in replies {
        match own.iter_mut().find(|o| o.2 == r.3) {
            Some(parent) => parent.0.replies.push(r.0),
            None => own.push(r),
        }
    }
    (
        own.into_iter().map(|r| (r.0, r.5)).collect(),
        context.into_iter().map(|r| r.0).collect(),
    )
}

/// The best new-person suggestion among a signal's senders and its author.
fn new_person(look: &Lookup, s: &Signal, author_ids: &[(String, String)]) -> Option<Suggestion> {
    if look.suggest.is_empty() {
        return None;
    }
    let mut accounts: Vec<(String, String)> = author_ids
        .iter()
        .filter_map(|(_, h)| people::account_of(&s.source, h))
        .collect();
    accounts.extend(
        s.author_handle
            .as_deref()
            .and_then(|h| people::account_of(&s.source, h)),
    );
    accounts
        .iter()
        .filter_map(|a| look.suggest.get(a))
        .max_by_key(|g| g.signals)
        .cloned()
}

fn item_author(look: &Lookup, s: &Signal) -> Option<Author> {
    let name = s.author_name.clone()?;
    Some(Author {
        who: look.people.who(&s.source, s.author_handle.as_deref(), &name),
        name,
    })
}

/// How much of each signal's messages to send.
#[derive(Clone, Copy, PartialEq)]
enum Bodies {
    /// Everything.
    All,
    /// Keys and counts only, except for agent signals (their part reads the
    /// last message): `list`, where a row's messages come with `messages`
    /// once you expand it.
    AgentsOnly,
}

fn load_messages(r: &dyn Reads, look: &Lookup, sigs: &mut [Signal], bodies: Bodies) -> Result<()> {
    let ids: Vec<i64> = sigs.iter().map(|s| s.item_id).collect();
    let full = |s: &Signal| bodies == Bodies::All || s.source == "agents";
    let (with_text, heads): (Vec<&Signal>, Vec<&Signal>) = sigs.iter().partition(|s| full(s));
    let mut messages = messages_by_item(r, &with_text.iter().map(|s| s.item_id).collect::<Vec<_>>())?;
    messages.extend(by_item(
        r.incoming()
            .message_heads(&heads.iter().map(|s| s.item_id).collect::<Vec<_>>())?,
    ));
    let mut held: HashMap<i64, super::todos::Held> = r.todos().held(&ids)?.into_iter().collect();
    let events: Vec<i64> = sigs.iter().filter(|s| s.kind == "event").map(|s| s.item_id).collect();
    let mut rsvps = super::respond::rsvps(r, &events, &look.clock)?;
    let chat: Vec<i64> = sigs.iter().filter(|s| is_chat(&s.source)).map(|s| s.item_id).collect();
    let mut replies = super::respond::replies(r, &chat, &look.clock)?;
    for s in sigs {
        let raw_rows = messages.remove(&s.item_id).unwrap_or_default();
        s.keys = raw_rows.iter().filter_map(|m| m.ext_key.clone()).collect();
        let rows = message_rows(look, &s.source, raw_rows);
        let author_ids: Vec<(String, String)> = rows
            .iter()
            .filter(|r| !r.1)
            .filter_map(|r| r.4.clone().map(|id| (r.0.author.clone(), id)))
            .collect();
        let (mut own, context) = nest(rows);
        let others = r.incoming().split_sibling_times(s.item_id)?;
        s.split = !others.is_empty();
        for i in 1..own.len() {
            let (prev, at) = (own[i - 1].1, own[i].1);
            own[i].0.gap = others.iter().any(|t| *t > prev && *t < at);
        }
        s.messages = own.into_iter().map(|r| r.0).collect();
        let triaged = s.messages.iter().any(|m| m.read || m.replies.iter().any(|r| r.read));
        s.context = if triaged && !s.thread { vec![] } else { context };
        s.n_messages = s.messages.len();
        if !full(s) {
            s.messages.clear();
            s.context.clear();
        }
        s.held = held.remove(&s.item_id);
        s.rsvp = rsvps.remove(&s.item_id);
        s.reply = replies.remove(&s.item_id);
        s.new_person = new_person(look, s, &author_ids);
        s.author = item_author(look, s);
    }
    Ok(())
}

/// An expanded row's messages, as `list` would have sent them, with the
/// revision and keys triage sends back for what you saw.
pub fn messages(r: &dyn Reads, clock: &Clock, item_id: i64, settings: &PeopleSettings) -> Result<Value> {
    let row = r.incoming().signal(item_id)?.ok_or_else(|| msg("no such item"))?;
    let look = Lookup::plain(r, settings, clock)?;
    let mut sigs = [to_signal(row, clock)];
    load_messages(r, &look, &mut sigs, Bodies::All)?;
    let [s] = sigs;
    Ok(json!({ "item_id": s.item_id, "rev": s.rev, "keys": s.keys, "messages": s.messages, "context": s.context }))
}

/// Topics with new signals (most recent signal first), then Unsorted, with
/// every signal's messages: what `list` and `messages` show together.
pub fn queue(r: &dyn Reads, clock: &Clock) -> Result<Queue> {
    queue_with(r, clock, &PeopleSettings::default(), Bodies::All)
}

/// `list`: the queue with message keys and counts but not the messages.
fn list(r: &dyn Reads, clock: &Clock, settings: &PeopleSettings) -> Result<Queue> {
    queue_with(r, clock, settings, Bodies::AgentsOnly)
}

fn queue_with(r: &dyn Reads, clock: &Clock, settings: &PeopleSettings, bodies: Bodies) -> Result<Queue> {
    let look = Lookup::load(r, settings, clock)?;
    let mut topics: Vec<TopicSignals> = vec![];
    for f in r.incoming().filed_signals()? {
        let sig = Signal {
            update: f.update,
            ..to_signal(f.signal, clock)
        };
        match topics.iter_mut().find(|t| t.id == f.topic_id) {
            Some(t) => t.signals.push(sig),
            None => topics.push(TopicSignals {
                id: f.topic_id,
                name: f.topic_name,
                category: f.topic_category,
                signals: vec![sig],
            }),
        }
    }
    // Most recent signal first (rows arrive newest first, so push order already is).
    topics.sort_by(|a, b| b.signals[0].at.cmp(&a.signals[0].at));
    for t in &mut topics {
        load_messages(r, &look, &mut t.signals, bodies)?;
    }

    let mut unsorted: Vec<Signal> = r
        .incoming()
        .unsorted_signals()?
        .into_iter()
        .map(|row| to_signal(row, clock))
        .collect();
    load_messages(r, &look, &mut unsorted, bodies)?;

    // Agents waiting (2b): every agent signal waiting on you, oldest first, with its topic.
    let mut agents: Vec<Signal> = r
        .incoming()
        .waiting_agents()?
        .into_iter()
        .map(|w| {
            let time = meta(
                &w.signal.title,
                w.signal.occurred_at,
                w.signal.ended_at,
                w.signal.msg_count,
                None,
                clock,
            );
            let mut sig = to_signal(w.signal, clock);
            sig.topic_id = w.topic.as_ref().map(|t| t.0);
            sig.topic = w.topic.map(|t| t.1);
            if let Some(a) = &sig.agent {
                sig.meta = agent_meta(a, Some(sig.topic.as_deref()), &time);
            }
            sig
        })
        .collect();
    load_messages(r, &look, &mut agents, Bodies::All)?;
    Ok(Queue {
        topics,
        unsorted,
        agents,
    })
}

/// File the signal under its suggested (or already confirmed) topic.
pub fn confirm(uow: &dyn UnitOfWork, now: Ts, item_id: i64, topic_id: i64) -> Result<()> {
    confirm_seen(uow, now, item_id, topic_id, None).map(|_| ())
}

/// `confirm` of what you saw (`Saw`). Returns whether the item had changed since.
pub fn confirm_seen(uow: &dyn UnitOfWork, now: Ts, item_id: i64, topic_id: i64, saw: Option<&Saw>) -> Result<bool> {
    if !uow.links().set_state(item_id, topic_id, "confirmed")? {
        return Err(msg("no suggested link to confirm"));
    }
    uow.items().set_seen(item_id, true)?;
    confirm_series(uow, item_id, topic_id)?;
    mark_read_remote(uow, item_id, topic_id, saw)?;
    // A todo held on the signal is made with the topic. (A Gmail thread
    // leaves the Inbox through filing itself, todo or not: `file_gmail`.)
    super::todos::settle(uow, now, item_id, topic_id)?;
    settle_saw(uow, item_id, saw)
}

/// A recurring event is triaged once: every instance of its series gets the topic.
fn confirm_series(uow: &dyn UnitOfWork, item_id: i64, topic_id: i64) -> Result<()> {
    let sibs = uow.items().series_siblings(item_id)?;
    if !sibs.is_empty() {
        uow.links().confirm(&sibs, topic_id, "same series")?;
        uow.items().set_seen_many(&sibs, true)?;
    }
    Ok(())
}

/// Confirming a chat (Slack) or Gmail signal also marks it read there, through the outbox.
/// Only items the chat connector stored with a read marker qualify (Slack
/// threads have none; Slack has no API for a thread's read state).
///
/// Split families (`split.rs`): `conversations.mark` up to a ts also marks
/// everything before it, and a run read in Slack closes the parent's item. So
/// while another part of the family is still new, nothing is marked; the last
/// part to leave Incoming marks the conversation up to the family's newest
/// marker.
fn mark_read_remote(uow: &dyn UnitOfWork, item_id: i64, topic_id: i64, saw: Option<&Saw>) -> Result<()> {
    let Some((source_id, kind, raws)) = settled_family(uow, item_id)? else {
        return Ok(());
    };
    match kind.as_str() {
        k if is_chat(k) => mark_chat_read(uow, source_id, k, item_id, &raws, saw),
        "gmail" => file_gmail(uow, source_id, item_id, &raws, topic_id, saw),
        _ => Ok(()),
    }
}

/// Source, kind and every family part's id and `raw_json`; `None` while another part of the family is still new.
fn settled_family(uow: &dyn UnitOfWork, item_id: i64) -> Result<Option<(i64, String, Vec<(i64, Value)>)>> {
    let family = uow.items().family(item_id)?;
    let Some(first) = family.first() else { return Ok(None) };
    if family.iter().any(|p| !p.seen && p.id != item_id) {
        return Ok(None);
    }
    let (source_id, kind) = (first.source_id, first.source_kind.clone());
    let raws = family
        .into_iter()
        .map(|p| (p.id, serde_json::from_str(&p.raw_json).unwrap_or_default()))
        .collect();
    Ok(Some((source_id, kind, raws)))
}

/// `<kind>.mark_read` up to the family's newest `raw_json.mark_ts` (chat
/// message keys sort as numbers, oldest first; `app/docs/chat.md`).
fn mark_chat_read(
    uow: &dyn UnitOfWork,
    source_id: i64,
    kind: &str,
    item_id: i64,
    raws: &[(i64, Value)],
    saw: Option<&Saw>,
) -> Result<()> {
    // A read-only source gets no remote op (the request would refuse it, after Confirm had done half its work).
    if crate::outbox::is_read_only(uow.reads(), source_id)? {
        return Ok(());
    }
    let ts = |k: &str| k.parse::<f64>().ok();
    let mut newest: Option<(f64, String, String)> = None;
    for (id, raw) in raws {
        let (Some(channel), Some(mut mark)) = (
            raw.get("channel").and_then(Value::as_str),
            raw.get("mark_ts").and_then(Value::as_str),
        ) else {
            continue;
        };
        // This item: no further than the newest of its messages you saw.
        if let (true, Some(saw)) = (*id == item_id, saw) {
            let msgs: Vec<&str> = raw
                .get("msgs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let seen = saw
                .keys
                .iter()
                .map(String::as_str)
                .filter(|k| msgs.contains(k) && ts(k) <= ts(mark))
                .max_by(|a, b| ts(a).partial_cmp(&ts(b)).unwrap_or(std::cmp::Ordering::Equal));
            match seen {
                Some(k) => mark = k,
                None => continue,
            }
        }
        let n = ts(mark).unwrap_or(0.0);
        if newest.as_ref().is_none_or(|b| n > b.0) {
            newest = Some((n, channel.to_owned(), mark.to_owned()));
        }
    }
    if let Some((_, channel, ts)) = newest {
        request(
            uow,
            source_id,
            &format!("{kind}.mark_read"),
            &json!({ "channel": channel, "ts": ts }),
            Some(item_id),
        )?;
    }
    Ok(())
}

/// A source's Gmail label for a category (`config.category_labels`).
fn category_label(config_json: &str, category: &str) -> Option<String> {
    let config: Value = serde_json::from_str(config_json).unwrap_or_default();
    config
        .pointer(&format!("/category_labels/{category}"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The thread, every Gmail message id the user saw in it (so `gmail.file` can spot mail that came later), and the
/// Gmail label of the topic's category (`config.category_labels`), if any.
fn file_gmail(
    uow: &dyn UnitOfWork,
    source_id: i64,
    item_id: i64,
    raws: &[(i64, Value)],
    topic_id: i64,
    saw: Option<&Saw>,
) -> Result<()> {
    let source = uow
        .reads()
        .sources()
        .get(source_id)?
        .ok_or_else(|| msg("no such source"))?;
    if crate::outbox::read_only_name(&source).is_some() {
        return Ok(());
    }
    let Some(thread) = raws
        .iter()
        .find_map(|(_, r)| r.get("thread_id").and_then(Value::as_str))
    else {
        return Ok(());
    };
    let mut ids: Vec<&str> = vec![];
    for id in raws
        .iter()
        .filter_map(|(_, r)| r.get("msgs").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
    {
        // Only mail you saw: anything later keeps the thread in the Inbox.
        if !ids.contains(&id) && saw.is_none_or(|s| s.keys.iter().any(|k| k == id)) {
            ids.push(id);
        }
    }
    let category = super::topic_ref(uow.reads(), topic_id)?.category;
    // Filing archives the thread, with or without a todo (user, 2026-10-01).
    let mut payload = json!({ "thread_id": thread, "message_ids": ids, "archive": true });
    if let Some(label) = category_label(&source.config_json, &category) {
        payload["label"] = json!(label);
    }
    request(uow, source_id, "gmail.file", &payload, Some(item_id))?;
    Ok(())
}

fn gmail_op(uow: &dyn UnitOfWork, item_id: i64, op: &str, saw: Option<&Saw>) -> Result<()> {
    match uow.items().get(item_id)? {
        Some(i) if i.source_kind == "gmail" && !crate::outbox::is_read_only(uow.reads(), i.source_id)? => {
            let mut payload = json!({ "thread_id": i.external_id });
            // Archive or trash only what you saw: newer unread mail keeps the thread.
            if let Some(saw) = saw {
                payload["message_ids"] = json!(saw.keys);
            }
            request(uow, i.source_id, op, &payload, Some(item_id))?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Manual split from Unsorted: the selected messages go to a new item confirmed
/// to `topic_id` (like `t` on a whole item). Selecting every message is just a
/// move of the whole item. Returns the item that got the topic.
pub fn split_to(uow: &dyn UnitOfWork, now: Ts, item_id: i64, message_ids: &[i64], topic_id: i64) -> Result<i64> {
    let mut ids = message_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut all = uow.splits().message_ids(item_id)?;
    all.sort_unstable();
    let target = if ids == all {
        item_id
    } else {
        uow.splits().carve(item_id, &ids, false)?
    };
    move_to(uow, now, target, topic_id, None)?;
    Ok(target)
}

/// Only records that the link was wrong. The item stays unseen, so with no
/// other live link it shows up in Unsorted.
pub fn wrong(uow: &dyn UnitOfWork, item_id: i64, topic_id: i64) -> Result<()> {
    if !uow.links().set_state(item_id, topic_id, "wrong")? {
        return Err(msg("no link to mark wrong"));
    }
    Ok(())
}

/// Out of triage without a topic; new activity brings it back. The source is only touched for Gmail (`gmail_op`).
pub fn dismiss(uow: &dyn UnitOfWork, item_id: i64, on: bool) -> Result<()> {
    if !uow.items().set_seen(item_id, on)? {
        return Err(msg("no such item"));
    }
    uow.todos().set_discarded(item_id, on)?;
    gmail_op(uow, item_id, if on { "gmail.archive" } else { "gmail.restore" }, None)
}

/// Dismiss what you saw (`Saw`). Returns whether the item had changed since.
pub fn dismiss_seen(uow: &dyn UnitOfWork, item_id: i64, saw: Option<&Saw>) -> Result<bool> {
    if !uow.items().set_seen(item_id, true)? {
        return Err(msg("no such item"));
    }
    uow.todos().set_discarded(item_id, true)?;
    gmail_op(uow, item_id, "gmail.archive", saw)?;
    settle_saw(uow, item_id, saw)
}

pub fn trash(uow: &dyn UnitOfWork, item_id: i64) -> Result<()> {
    trash_seen(uow, item_id, None).map(|_| ())
}

/// Trash what you saw (`Saw`). Returns whether the item had changed since.
pub fn trash_seen(uow: &dyn UnitOfWork, item_id: i64, saw: Option<&Saw>) -> Result<bool> {
    let item = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?;
    if item.source_kind != "gmail" {
        return Err(msg("only Gmail threads can be trashed"));
    }
    uow.items().set_seen(item_id, true)?;
    uow.todos().set_discarded(item_id, true)?;
    gmail_op(uow, item_id, "gmail.trash", saw)?;
    settle_saw(uow, item_id, saw)
}

/// The user picked the topic, so it's confirmed. If the item came from another
/// topic's suggestion, that suggestion was wrong.
pub fn move_to(uow: &dyn UnitOfWork, now: Ts, item_id: i64, topic_id: i64, from: Option<i64>) -> Result<()> {
    move_seen(uow, now, item_id, topic_id, from, None).map(|_| ())
}

/// `move_to` of what you saw (`Saw`). Returns whether the item had changed since.
pub fn move_seen(
    uow: &dyn UnitOfWork,
    now: Ts,
    item_id: i64,
    topic_id: i64,
    from: Option<i64>,
    saw: Option<&Saw>,
) -> Result<bool> {
    if let Some(f) = from.filter(|f| *f != topic_id) {
        wrong(uow, item_id, f)?;
    }
    uow.links().confirm(&[item_id], topic_id, "picked by you")?;
    uow.items().set_seen(item_id, true)?;
    confirm_series(uow, item_id, topic_id)?;
    if let Some((source_id, kind, raws)) = settled_family(uow, item_id)? {
        if kind == "gmail" {
            file_gmail(uow, source_id, item_id, &raws, topic_id, saw)?;
        }
    }
    super::todos::settle(uow, now, item_id, topic_id)?;
    settle_saw(uow, item_id, saw)
}

// ---------------------------------------------------------------------------
// Split review (15g, 15h).

pub use crate::domain::incoming::TopicRef as PartTopic;

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

/// What a chat item's conversation is: `raw_json.conv` when its connector
/// says (`app/docs/chat.md`), else read off Slack's titles.
fn conv_kind(title: &str, raw: &Value) -> &'static str {
    let said = match raw.get("conv").and_then(Value::as_str) {
        Some("dm") => Some("dm"),
        Some("group") => Some("group"),
        Some("channel") => Some("channel"),
        _ => None,
    };
    if raw.get("thread_ts").is_some() {
        "thread"
    } else if let Some(k) = said {
        k
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

/// The parts of `item_id`'s family the review works on: those still in
/// Incoming (or, reviewing filed signals, the parts in `scope`), plus the
/// family's dismissed messages.
fn review_parts(r: &dyn Reads, item_id: i64, scope: Option<&[i64]>) -> Result<(i64, Vec<Part>)> {
    let (root, ids) = r.incoming().family(item_id)?;
    let parts = r
        .incoming()
        .parts(&ids)?
        .into_iter()
        .filter(|p| {
            let wanted = match scope {
                Some(parts) => parts.contains(&p.item_id),
                None => !p.seen,
            };
            (p.dismissed && p.has_messages) || (!p.dismissed && wanted)
        })
        .map(|p| Part {
            item_id: p.item_id,
            topic: if p.dismissed { None } else { p.topic },
            dismissed: p.dismissed,
        })
        .collect();
    Ok((root, parts))
}

pub fn family(
    r: &dyn Reads,
    clock: &Clock,
    item_id: i64,
    scope: Option<&[i64]>,
    settings: &PeopleSettings,
) -> Result<Family> {
    let (root, parts) = review_parts(r, item_id, scope)?;
    let h = r.incoming().item_header(item_id)?.ok_or_else(|| msg("no such item"))?;
    let look = Lookup::plain(r, settings, clock)?;
    let ids: Vec<i64> = parts.iter().map(|p| p.item_id).collect();
    let mut messages = messages_by_item(r, &ids)?;
    let mut all: Vec<(Message, Ts)> = vec![];
    for p in &parts {
        let (own, _) = nest(message_rows(
            &look,
            &h.source_kind,
            messages.remove(&p.item_id).unwrap_or_default(),
        ));
        all.extend(own.into_iter().map(|(m, at)| {
            (
                Message {
                    part: Some(p.item_id),
                    ..m
                },
                at,
            )
        }));
    }
    all.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.id.cmp(&b.0.id)));
    let raw: Value = serde_json::from_str(&h.raw_json).unwrap_or_default();
    Ok(Family {
        root,
        conv: conv_kind(&h.title, &raw),
        title: h.title,
        source: h.source_kind,
        url: h.url,
        parts,
        messages: all.into_iter().map(|(m, _)| m).collect(),
    })
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
fn settle_parts(uow: &dyn UnitOfWork, parts: &[i64]) -> Result<()> {
    for p in uow.reads().incoming().parts(parts)? {
        if !p.has_messages {
            uow.splits().prune(p.item_id)?;
        } else if p.dismissed {
            uow.items().set_seen(p.item_id, true)?;
        } else if !p.filed {
            uow.items().set_seen(p.item_id, false)?;
        }
    }
    Ok(())
}

/// Move messages of one part to another part, a topic or Unsorted. Returns
/// the part they're in now. Reviewing filed signals (`scope`), a new part is
/// filed straight away (confirmed), not suggested.
pub fn review_move(uow: &dyn UnitOfWork, from: i64, ids: &[i64], to: To, scope: Option<&[i64]>) -> Result<i64> {
    let target = match to {
        To::Part(part) => part,
        To::Topic(_) | To::Unsorted => {
            let topic = match to {
                To::Topic(t) => Some(t),
                _ => None,
            };
            let (_, parts) = review_parts(uow.reads(), from, scope)?;
            match parts
                .iter()
                .find(|p| !p.dismissed && p.topic.as_ref().map(|t| t.id) == topic)
            {
                Some(p) => p.item_id,
                None => return new_part(uow, from, ids, topic, scope.is_some()),
            }
        }
    };
    if target != from {
        uow.splits().move_messages(from, ids, target)?;
        settle_parts(uow, &[target, from])?;
    }
    Ok(target)
}

/// A new part of the family holding `ids`, linked to `topic` (None:
/// Unsorted): suggested in Incoming's review, confirmed (`filed`) when
/// reclassifying a filed signal.
fn new_part(uow: &dyn UnitOfWork, from: i64, ids: &[i64], topic: Option<i64>, filed: bool) -> Result<i64> {
    let child = uow.splits().carve(from, ids, true)?;
    uow.items().set_dismissed(child, false)?;
    uow.items().set_seen(child, filed && topic.is_some())?;
    if let Some(t) = topic {
        uow.links()
            .pick(child, t, if filed { "confirmed" } else { "suggested" })?;
    }
    settle_parts(uow, &[from, child])?;
    Ok(child)
}

/// Dismiss single messages in place: they join the family's dismissed part
/// (made on first use), which never shows in Incoming. Returns that part.
pub fn review_dismiss(uow: &dyn UnitOfWork, from: i64, ids: &[i64]) -> Result<i64> {
    let (_, family) = uow.reads().incoming().family(from)?;
    let bin = uow
        .reads()
        .incoming()
        .parts(&family)?
        .into_iter()
        .find(|p| p.dismissed)
        .map(|p| p.item_id);
    let bin = match bin {
        Some(b) if b != from => {
            uow.splits().move_messages(from, ids, b)?;
            b
        }
        Some(b) => b,
        None => {
            let child = uow.splits().carve(from, ids, true)?;
            uow.items().set_dismissed(child, true)?;
            uow.links().wrong_all(child)?;
            child
        }
    };
    settle_parts(uow, &[bin, from])?;
    Ok(bin)
}

/// Confirm every part in the review that has a topic. Unsorted stays in Incoming.
pub fn review_confirm(uow: &dyn UnitOfWork, now: Ts, item_id: i64) -> Result<usize> {
    let (_, parts) = review_parts(uow.reads(), item_id, None)?;
    let mut n = 0;
    for p in parts.iter().filter(|p| !p.dismissed) {
        if let Some(t) = &p.topic {
            confirm(uow, now, p.item_id, t.id)?;
            n += 1;
        }
    }
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
    /// Its unread messages, so `u` makes the same ones unread again.
    #[serde(default)]
    pub unread: Vec<String>,
}

/// The items moving together: a recurring event's whole series, else the item.
fn series_of(uow: &dyn UnitOfWork, item_id: i64) -> Result<Vec<i64>> {
    let mut ids = uow.items().series_siblings(item_id)?;
    ids.push(item_id);
    ids.sort_unstable();
    Ok(ids)
}

/// A Gmail thread filed under a topic of another category: swap its category
/// label (`config.category_labels`), through the outbox.
pub fn relabel_gmail(uow: &dyn UnitOfWork, item_id: i64, from_category: Option<&str>, to_category: &str) -> Result<()> {
    if from_category == Some(to_category) {
        return Ok(());
    }
    let Some(item) = uow.items().get(item_id)? else {
        return Ok(());
    };
    let Some(source) = uow.reads().sources().get(item.source_id)? else {
        return Ok(());
    };
    if item.source_kind != "gmail" || crate::outbox::read_only_name(&source).is_some() {
        return Ok(());
    }
    let raw: Value = serde_json::from_str(&item.raw_json).unwrap_or_default();
    let Some(thread) = raw.get("thread_id").and_then(Value::as_str) else {
        return Ok(());
    };
    let label = |c: &str| category_label(&source.config_json, c);
    let (remove, add) = (from_category.and_then(label), label(to_category));
    if remove.is_none() && add.is_none() {
        return Ok(());
    }
    request(
        uow,
        source.id,
        "gmail.relabel",
        &json!({ "thread_id": thread, "remove": remove, "add": add }),
        Some(item_id),
    )?;
    Ok(())
}

fn category_of(uow: &dyn UnitOfWork, topic: i64) -> Result<String> {
    Ok(super::topic_ref(uow.reads(), topic)?.category)
}

/// Move a signal to another topic (`t` in the chronology or search). One
/// topic per signal: the old link becomes `wrong`, the new one `confirmed`,
/// filed straight away. Nothing is written to the source, except a Gmail
/// thread's category label when the category changes. Returns what to undo.
pub fn reclassify(uow: &dyn UnitOfWork, now: Ts, item_id: i64, from: Option<i64>, to: i64) -> Result<Vec<Moved>> {
    if from == Some(to) {
        return Ok(vec![]);
    }
    let from_cat = from.map(|f| category_of(uow, f)).transpose()?;
    let to_cat = category_of(uow, to)?;
    let mut moved = vec![];
    for id in series_of(uow, item_id)? {
        let seen = uow.items().get(id)?.ok_or_else(|| msg("no such item"))?.seen;
        let m = Moved {
            item_id: id,
            from_state: from.map(|f| uow.links().state(id, f)).transpose()?.flatten(),
            to_state: uow.links().state(id, to)?,
            seen,
            unread: uow.items().unread_keys(id)?,
        };
        if let (Some(f), Some(_)) = (from, &m.from_state) {
            uow.links().restore(id, f, Some("wrong"))?;
        }
        uow.links().restore(id, to, Some("confirmed"))?;
        uow.items().set_seen(id, true)?;
        moved.push(m);
    }
    // Still in Incoming (from search): that's filing, so a Gmail thread is
    // labelled, marked read and archived as Confirm would. Filed already: only
    // its category label changes.
    let was_new = moved.iter().any(|m| m.item_id == item_id && !m.seen);
    match (was_new, settled_family(uow, item_id)?) {
        (true, Some((source_id, kind, raws))) if kind == "gmail" => {
            file_gmail(uow, source_id, item_id, &raws, to, None)?
        }
        _ => relabel_gmail(uow, item_id, from_cat.as_deref(), &to_cat)?,
    }
    super::todos::settle(uow, now, item_id, to)?;
    Ok(moved)
}

/// The links and seen flags `unreclassify` needs, read before an action changes them.
fn snapshot(uow: &dyn UnitOfWork, item_id: i64, from: Option<i64>, to: i64) -> Result<Vec<Moved>> {
    series_of(uow, item_id)?
        .into_iter()
        .map(|id| {
            let seen = uow.items().get(id)?.ok_or_else(|| msg("no such item"))?.seen;
            Ok(Moved {
                item_id: id,
                from_state: from.map(|f| uow.links().state(id, f)).transpose()?.flatten(),
                to_state: uow.links().state(id, to)?,
                seen,
                unread: uow.items().unread_keys(id)?,
            })
        })
        .collect()
}

/// `u` after Confirm, Move or Wrong in Incoming: the links and seen flags come back. `filed`: a Gmail thread goes back
/// to the Inbox. (A Slack read marker and a todo the filing made stay.)
pub fn unfile(uow: &dyn UnitOfWork, moved: &[Moved], from: Option<i64>, to: i64, filed: bool) -> Result<()> {
    unreclassify(uow, moved, from, to)?;
    match moved.first() {
        Some(m) if filed => gmail_op(uow, m.item_id, "gmail.restore", None),
        _ => Ok(()),
    }
}

/// `u` after a reclassify: every link back as it was.
pub fn unreclassify(uow: &dyn UnitOfWork, moved: &[Moved], from: Option<i64>, to: i64) -> Result<()> {
    for m in moved {
        uow.links().restore(m.item_id, to, m.to_state.as_deref())?;
        if let Some(f) = from {
            uow.links().restore(m.item_id, f, m.from_state.as_deref())?;
        }
        uow.items().set_seen(m.item_id, m.seen)?;
        uow.items().mark_unread(m.item_id, &m.unread)?;
    }
    if let (Some(first), Some(f)) = (moved.first(), from) {
        let from_cat = category_of(uow, f)?;
        relabel_gmail(uow, first.item_id, Some(&category_of(uow, to)?), &from_cat)?;
    }
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

pub fn detail(r: &dyn Reads, clock: &Clock, item_id: i64, settings: &PeopleSettings) -> Result<Detail> {
    let h = r
        .incoming()
        .item_header(item_id)?
        .filter(|h| h.work)
        .ok_or_else(|| msg("no such item"))?;
    let look = Lookup::plain(r, settings, clock)?;
    let rows = messages_by_item(r, &[item_id])?.remove(&item_id).unwrap_or_default();
    let (own, mut context) = nest(message_rows(&look, &h.source_kind, rows));
    Ok(Detail {
        item_id,
        source: h.source_kind,
        url: h.url,
        summary: h.summary,
        msg_count: h.msg_count,
        messages: own.into_iter().map(|(m, _)| m).collect(),
        context: context.pop(),
    })
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
    for (i, (t, who, text)) in [
        (
            "09:05",
            "Jordan",
            "Root cause looks like a retry storm from the v2 delivery workers.",
        ),
        ("09:31", "Marco R.", "Backoff patch is up, needs a second reviewer."),
        ("10:02", "Sam", "Acme is asking for an exec on a call today."),
        ("10:40", "Jordan", "Fix ETA 14:00. Backlog is draining."),
    ]
    .iter()
    .enumerate()
    {
        ctx.slack_keyed(inc, ctx.at(0, t), who, text, Some(&format!("C2291.{i}")), None);
    }
    ctx.sample().patch_item(
        inc,
        &crate::domain::sample::ItemPatch {
            raw_json: Some(&json!({ "channel": "C2291" }).to_string()),
            ..Default::default()
        },
    )?;
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
    // An invite you haven't answered, overlapping DD prep and a Life block (14a).
    let invite = suggest(
        ItemSeed {
            source: "gcal",
            kind: "event",
            title: "Acme postmortem review".into(),
            summary: Some("Sam, Dana · needs your response".into()),
            url: Some("https://calendar.google.com/calendar/r/day".into()),
            author: Some("Sam"),
            occurred_at: ctx.at(0, "17:00"),
            ended_at: Some(ctx.at(0, "18:00")),
            ..Default::default()
        },
        acme,
    );
    ctx.invite(invite)?;
    ctx.sample().event(&crate::domain::sample::SampleEvent {
        item_id: Some(invite),
        source_id: ctx.source("gcal"),
        start: ctx.at(0, "17:00"),
        end: ctx.at(0, "18:00"),
        title: Some("Acme postmortem review"),
        category: "work",
        tentative: true,
        attendees: Some("Sam, Dana"),
        life: false,
    })?;

    // Alerting · P2: the alert mail, grouped per alert rule. Each group is one
    // signal whose msg_count is the number of alert mails behind it (214 total).
    for (rule, count, from, to, sample) in [
        (
            "disk-pressure · node-7",
            168,
            "00:14",
            "10:36",
            "[P2] disk-pressure node-7: /var at 91%",
        ),
        (
            "queue-lag · ingest-eu",
            31,
            "06:02",
            "10:31",
            "[P2] queue-lag ingest-eu: 4m12s behind",
        ),
        (
            "cert-expiry · api-gw staging",
            15,
            "07:00",
            "10:00",
            "[P2] cert-expiry api-gw-staging: 13 days left",
        ),
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
        ctx.message(id, ctx.at(0, from), "alerts", sample);
        ctx.message(id, ctx.at(0, to), "alerts", sample);
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
        (
            "09:13",
            "They want to wait until after their code freeze on Oct 20. OK to push PLAT-1170 out?",
        ),
        (
            "09:16",
            "Different thing: Lee asked me for data eng hiring numbers for the Q4 model.",
        ),
        ("09:20", "Can you look at the offsite agenda before Thursday?"),
    ] {
        ctx.slack(dm, ctx.at(0, t), "Priya", text);
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
    ctx.slack(
        eng,
        ctx.at(0, "08:12"),
        "Marco R.",
        "Anyone seen the staging deploy hang on migrations?",
    );
    // His Slack profile (users.info), so the strip reads as in 17a.
    ctx.sample().profile(
        "slack",
        &crate::seed::slack_id("Marco R."),
        "Marco Rossi",
        "marco.rossi@work.com",
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
        let raw = json!({ "channel": channel, "msgs": keys, "mark_ts": keys.last() }).to_string();
        ctx.sample().patch_item(
            root,
            &crate::domain::sample::ItemPatch {
                raw_json: Some(&raw),
                ..Default::default()
            },
        )?;
        let mut groups: Vec<(G, Vec<i64>)> = vec![];
        for ((t, who, text, g, replies), key) in msgs.iter().zip(&keys) {
            let id = ctx.slack_keyed(root, ctx.at(0, t), who, text, Some(key), None);
            for (i, (rt, rwho, rtext)) in replies.iter().enumerate() {
                ctx.slack_keyed(
                    root,
                    ctx.at(0, rt),
                    rwho,
                    rtext,
                    Some(&format!("{key}.r{i}")),
                    Some(key),
                );
            }
            match groups.iter_mut().find(|x| x.0 == *g) {
                Some(x) => x.1.push(id),
                None => groups.push((*g, vec![id])),
            }
        }
        ctx.uow.splits().recount(root, 0)?;
        for (g, ids) in groups {
            let item = if g == G::Acme {
                root
            } else {
                ctx.uow.splits().carve(root, &ids, false)?
            };
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
            (
                "09:12",
                "Jordan",
                "Retry storm is contained, backlog is down to 3k.",
                Acme,
                &[],
            ),
            ("09:13", "Zach", "Nice. Who's writing the RCA?", Acme, &[]),
            (
                "09:14",
                "Jordan",
                "Also, can we talk about the SRE req before Wed?",
                Q4,
                &[],
            ),
            ("09:15", "Jordan", "Priya said it might move to Q1.", Q4, &[]),
            ("09:16", "Zach", "Yes. Tue 15:00?", Q4, &[]),
            (
                "09:40",
                "Jordan",
                "Acme wants the RCA by Friday. I can draft it.",
                Acme,
                &[],
            ),
            ("09:41", "Jordan", "Who should review, you or Marco?", Acme, &[]),
            ("10:02", "Jordan", "lunch?", Unsorted, &[]),
            ("10:03", "Zach", "can't today, tomorrow?", Unsorted, &[]),
            (
                "10:04",
                "Jordan",
                "Also the Acme dashboards are still red.",
                Unsorted,
                &[],
            ),
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
            (
                "09:30",
                "Priya",
                "Unrelated, but is the SRE req still on for Q4?",
                Q4,
                &[],
            ),
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
    use rusqlite::params;
    use rusqlite::Connection;

    fn link_state(conn: &Connection, item: i64, topic: i64) -> Result<Option<String>> {
        crate::store::over(conn).links().state(item, topic)
    }

    /// Sample data seeded on Sep 27 2026 at 10:52, in UTC.
    fn setup() -> (Connection, Clock, tempfile::TempDir) {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let now = crate::sync::test_clock("2026-09-27T10:52:00Z");
        crate::seed::seed(&crate::store::over(&conn), &now, dir.path()).unwrap();
        (conn, now, dir)
    }

    fn topic(q: &Queue, name: &str) -> Option<usize> {
        q.topics.iter().position(|t| t.name == name)
    }

    fn unseen(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM items WHERE seen = 0", [], |r| r.get(0))
            .unwrap()
    }

    fn shown(q: &Queue) -> usize {
        q.topics.iter().map(|t| t.signals.len()).sum::<usize>() + q.unsorted.len()
    }

    #[test]
    fn events_say_when_and_what_changed() {
        let now = &crate::sync::test_clock("2026-09-28T10:00:00Z");
        assert_eq!(
            event_meta(
                "2026-10-01T17:00:00Z".into(),
                Some("2026-10-01T19:00:00Z".into()),
                Some("Sam"),
                now
            ),
            "Sam · Thu 17:00–19:00"
        );
        assert_eq!(
            event_meta(
                "2026-10-01T00:00:00Z".into(),
                Some("2026-10-03T00:00:00Z".into()),
                None,
                now
            ),
            "Thu–Fri"
        );
        assert_eq!(
            event_meta(
                "2026-09-28T00:00:00Z".into(),
                Some("2026-09-29T00:00:00Z".into()),
                None,
                now
            ),
            "Today"
        );
        assert_eq!(
            change_label("moved", Some("2026-09-29T14:00:00Z".into()), now).as_deref(),
            Some("Moved from Tue 14:00")
        );
        assert_eq!(change_label("cancelled", None, now).as_deref(), Some("Cancelled"));
        // Another zone, the same instants: the labels follow the wall clock there.
        let la = &Clock::fixed(
            crate::time::t("2026-09-28T17:00:00"),
            jiff::tz::TimeZone::get("America/Los_Angeles").unwrap(),
        );
        assert_eq!(
            event_meta(
                "2026-10-02T00:00:00Z".into(),
                Some("2026-10-02T02:00:00Z".into()),
                Some("Sam"),
                la
            ),
            "Sam · Thu 17:00–19:00"
        );
        assert_eq!(excerpt("abcdef", 3), "abc…");
    }

    #[test]
    fn the_list_leaves_messages_to_the_row_you_expand() {
        let (conn, now, _d) = setup();
        let r = crate::store::over(&conn);
        let people = PeopleSettings::default();
        let all = |q: &Queue| {
            q.topics
                .iter()
                .flat_map(|t| &t.signals)
                .chain(&q.unsorted)
                .chain(&q.agents)
                .map(|s| json!(s))
                .collect::<Vec<_>>()
        };
        let (full, lazy) = (all(&queue(&r, &now).unwrap()), all(&list(&r, &now, &people).unwrap()));
        assert_eq!(full.len(), lazy.len());
        let mut lazy_rows = 0;
        for (f, l) in full.iter().zip(&lazy) {
            assert_eq!(f["item_id"], l["item_id"]);
            // What triage sends back, and what a collapsed row shows, are the same.
            for k in [
                "rev",
                "keys",
                "n_messages",
                "split",
                "new_person",
                "author",
                "held",
                "meta",
            ] {
                assert_eq!(f[k], l[k], "{k} of item {}", f["item_id"]);
            }
            assert_eq!(f["n_messages"], f["messages"].as_array().unwrap().len());
            if l["source"] == "agents" {
                assert_eq!(f["messages"], l["messages"], "agent signals keep theirs");
                continue;
            }
            assert_eq!(l["messages"], json!([]));
            assert_eq!(l["context"], json!([]));
            // Expanding it brings exactly what the full queue had.
            let m = messages(&r, &now, l["item_id"].as_i64().unwrap(), &people).unwrap();
            for k in ["rev", "keys", "messages", "context"] {
                assert_eq!(m[k], f[k], "{k} of item {}", f["item_id"]);
            }
            lazy_rows += f["n_messages"].as_u64().unwrap();
        }
        assert!(lazy_rows > 0, "the sample data has signals with messages");
        assert!(messages(&r, &now, 999_999, &people).is_err());
    }

    #[test]
    fn seed_matches_mock() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let names: Vec<_> = q.topics.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Acme outage",
                "Alerting · P2",
                "Q4 headcount",
                "v1 ingest deprecation",
                "Career"
            ]
        );
        let acme = &q.topics[0];
        assert_eq!(
            acme.signals.len(),
            5,
            "inc-2291, the two split Slack signals (15g, 15h), mail, invite"
        );
        let slack = acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap();
        assert_eq!(slack.meta, "42 new · 09:05–10:40");
        assert_eq!(slack.messages.len(), 4);
        let cal = acme.signals.iter().find(|s| s.source == "gcal").unwrap();
        assert_eq!(cal.meta, "Sam · Today 17:00–18:00");
        let rsvp = cal.rsvp.as_ref().expect("an unanswered invite offers Accept/Decline");
        assert_eq!(rsvp.status, "needs_action");
        assert_eq!(
            rsvp.overlaps,
            ["DD prep 16:45–17:15", "Life 17:30–18:00"],
            "Life blocks never show a title"
        );
        assert!(rsvp.answer.is_none());
        let mail = acme.signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail.meta, "10:22");
        let alerts: i64 = q.topics[1].signals.iter().filter_map(|s| s.msg_count).sum();
        assert_eq!(alerts, 214);
        assert_eq!(
            q.unsorted.len(),
            9,
            "6 items, the Unsorted parts of both splits, #platform-eng"
        );
        assert!(q.unsorted.iter().any(|s| s.source == "github"));
        assert!(q.unsorted.iter().any(|s| s.source == "jira"));
        // The tab badge (unseen items) matches what the queue shows.
        assert_eq!(unseen(&conn) as usize, shown(&q));
    }

    #[test]
    fn a_triaged_channel_run_drops_the_channel_context() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let s = q
            .topics
            .iter()
            .flat_map(|t| &t.signals)
            .chain(&q.unsorted)
            .find(|s| s.source == "slack" && !s.split && !s.messages.is_empty())
            .unwrap();
        let (id, first) = (s.item_id, s.messages[0].id);
        conn.execute("INSERT INTO item_messages (item_id, at, author, text, context) VALUES (?1, '2000-01-01T00:00:00.000Z', 'x', 'earlier in the channel', 1)", [id]).unwrap();
        conn.execute(
            "UPDATE items SET raw_json = json_remove(raw_json, '$.thread_ts') WHERE id = ?1",
            [id],
        )
        .unwrap();
        let find = |q: &Queue| {
            q.topics
                .iter()
                .flat_map(|t| &t.signals)
                .chain(&q.unsorted)
                .find(|x| x.item_id == id)
                .map(|x| x.context.len())
                .unwrap()
        };
        assert!(
            find(&queue(&crate::store::over(&conn), &now).unwrap()) > 0,
            "nothing read yet: context shows"
        );
        conn.execute("UPDATE item_messages SET read = 1 WHERE id = ?1", [first])
            .unwrap();
        assert_eq!(
            find(&queue(&crate::store::over(&conn), &now).unwrap()),
            0,
            "already read: no channel context"
        );
        conn.execute(
            "UPDATE items SET raw_json = json_set(raw_json, '$.thread_ts', '1.0') WHERE id = ?1",
            [id],
        )
        .unwrap();
        assert!(
            find(&queue(&crate::store::over(&conn), &now).unwrap()) > 0,
            "a thread keeps its parent as context"
        );
    }

    #[test]
    fn gmail_signal_carries_its_gmail_category() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let mail = q.topics[0].signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail.category, None, "no category stored yet");
        conn.execute(
            "UPDATE items SET raw_json = json_set(raw_json, '$.category', 'vendors') WHERE id = ?1",
            [mail.item_id],
        )
        .unwrap();
        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        let mail2 = q2.topics[0].signals.iter().find(|s| s.source == "gmail").unwrap();
        assert_eq!(mail2.category.as_deref(), Some("vendors"));
    }

    #[test]
    fn confirm_marks_seen_and_confirmed() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let t = &q.topics[0];
        let item = t.signals[0].item_id;
        confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            item,
            t.id,
        )
        .unwrap();
        assert_eq!(link_state(&conn, item, t.id).unwrap().as_deref(), Some("confirmed"));
        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        assert_eq!(q2.topics[topic(&q2, "Acme outage").unwrap()].signals.len(), 4);
        assert!(!q2.unsorted.iter().any(|s| s.item_id == item));
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    #[test]
    fn updates_on_filed_items_show_under_their_topic() {
        let (conn, now, _d) = setup();
        let (item, topic): (i64, i64) = conn
            .query_row(
                "SELECT item_id, topic_id FROM topic_links WHERE state = 'confirmed' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [item]).unwrap();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let t = q.topics.iter().find(|t| t.id == topic).unwrap();
        assert!(t.signals.iter().any(|s| s.item_id == item && s.update));
        assert_eq!(unseen(&conn) as usize, shown(&q));
        confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            item,
            topic,
        )
        .unwrap();
        assert_eq!(
            unseen(&conn) as usize,
            shown(&queue(&crate::store::over(&conn), &now).unwrap())
        );
    }

    #[test]
    fn dismiss_leaves_triage_and_undo_brings_it_back() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let item = q.unsorted[0].item_id;
        dismiss(&crate::store::over(&conn), item, true).unwrap();
        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(!q2.unsorted.iter().any(|s| s.item_id == item));
        assert_eq!(unseen(&conn) as usize, shown(&q2));
        dismiss(&crate::store::over(&conn), item, false).unwrap();
        assert!(queue(&crate::store::over(&conn), &now)
            .unwrap()
            .unsorted
            .iter()
            .any(|s| s.item_id == item));
    }

    #[test]
    fn undoing_a_dismiss_leaves_the_same_messages_unread() {
        let (conn, now, _d) = setup();
        let item = slack_run(&conn, &["100.1", "100.2", "100.3"]);
        conn.execute(
            "UPDATE item_messages SET read = 1 WHERE item_id = ?1 AND ext_key = '100.1'",
            [item],
        )
        .unwrap();
        let sql = crate::store::over(&conn);
        let uow: &dyn UnitOfWork = &sql;
        let before = uow.items().unread_keys(item).unwrap();
        assert_eq!(before.len(), 2);
        let saw = listed(&conn, &now, item);
        dismiss_seen(uow, item, Some(&saw)).unwrap();
        assert!(uow.items().unread_keys(item).unwrap().is_empty());
        dismiss(uow, item, false).unwrap();
        uow.items().mark_unread(item, &before).unwrap();
        assert_eq!(uow.items().unread_keys(item).unwrap(), before);
    }

    #[test]
    fn trash_sets_a_held_todo_aside_like_dismiss_and_undo_holds_it_again() {
        let (conn, _now, _d) = setup();
        let item: i64 = conn
            .query_row(
                "SELECT i.id FROM items i JOIN sources s ON s.id = i.source_id WHERE s.kind = 'gmail' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let sql = crate::store::over(&conn);
        let uow: &dyn UnitOfWork = &sql;
        let todo = crate::domain::todos::Held {
            target: "local".into(),
            text: "Reply to Legal".into(),
            due: None,
            sched: None,
        };
        uow.todos().hold(item, &todo).unwrap();
        trash(uow, item).unwrap();
        assert!(
            uow.todos().held(item).unwrap().is_none(),
            "a trashed thread's held todo is set aside"
        );
        dismiss(uow, item, false).unwrap();
        assert_eq!(
            uow.todos().held(item).unwrap().map(|h| h.text).as_deref(),
            Some("Reply to Legal")
        );
    }

    #[test]
    fn wrong_drops_to_unsorted_without_deleting() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let career = &q.topics[topic(&q, "Career").unwrap()];
        let item = career.signals[0].item_id;
        let before: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        wrong(&crate::store::over(&conn), item, career.id).unwrap();
        assert_eq!(link_state(&conn, item, career.id).unwrap().as_deref(), Some("wrong"));
        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(
            topic(&q2, "Career").is_none(),
            "topic with no signals left drops off the list"
        );
        assert!(q2.unsorted.iter().any(|s| s.item_id == item));
        let after: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
        assert_eq!(unseen(&conn) as usize, shown(&q2));
        assert!(wrong(&crate::store::over(&conn), item, 9999).is_err());
    }

    #[test]
    fn move_from_unsorted_confirms_picked_topic() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let pr = q.unsorted.iter().find(|s| s.source == "github").unwrap().item_id;
        let target: i64 = conn
            .query_row("SELECT id FROM topics WHERE name = 'Webhook reliability'", [], |r| {
                r.get(0)
            })
            .unwrap();
        move_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            pr,
            target,
            None,
        )
        .unwrap();
        assert_eq!(link_state(&conn, pr, target).unwrap().as_deref(), Some("confirmed"));
        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        assert_eq!(q2.unsorted.len(), 8);
        assert_eq!(unseen(&conn) as usize, shown(&q2));
    }

    fn topic_named(conn: &Connection, name: &str) -> i64 {
        conn.query_row("SELECT id FROM topics WHERE name = ?1", [name], |r| r.get(0))
            .unwrap()
    }

    fn exists(conn: &Connection, item: i64) -> bool {
        conn.query_row("SELECT EXISTS (SELECT 1 FROM items WHERE id = ?1)", [item], |r| {
            r.get(0)
        })
        .unwrap()
    }

    #[test]
    fn messages_carry_people_and_unknown_senders_get_a_suggestion() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let inc = acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap();
        let jordan = &inc.messages[0].who;
        assert!(jordan.known && jordan.name.as_deref() == Some("Jordan"));
        assert_eq!(jordan.color, people::hash_color("jordan.lee@work.com"));
        let marco = &inc.messages[1].who;
        assert!(!marco.known);
        assert_eq!(marco.account.as_ref().map(|a| a.id.as_str()), Some("U0MARCO"));
        assert_eq!(
            marco.color,
            people::hash_color("marco.rossi@work.com"),
            "coloured by his profile email"
        );
        let np = inc.new_person.as_ref().expect("Marco is in 3 signals today");
        assert_eq!(
            (np.account.as_str(), np.name.as_str(), np.signals),
            ("U0MARCO", "Marco R.", 3)
        );
        assert_eq!(
            (np.real_name.as_deref(), np.email.as_deref()),
            (Some("Marco Rossi"), Some("marco.rossi@work.com"))
        );
        let invite = acme.signals.iter().find(|s| s.source == "gcal").unwrap();
        let author = invite.author.as_ref().expect("the invite's organiser");
        assert_eq!(
            (author.name.as_str(), author.who.known),
            ("Sam", true),
            "matched like a message sender"
        );
        assert!(invite.meta.starts_with("Sam · "));
        let dm = acme.signals.iter().find(|s| s.title == "DM · Jordan").unwrap();
        assert!(dm.messages[1].who.me, "your own messages are you, in your colour");
        assert_eq!(dm.messages[1].who.color, 0);
        assert!(dm.new_person.is_none());
    }

    #[test]
    fn a_split_signal_shows_where_its_other_parts_came_in() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let dm = acme.signals.iter().find(|s| s.title == "DM · Jordan").unwrap();
        assert!(dm.split);
        let times: Vec<(&str, bool)> = dm.messages.iter().map(|m| (m.time.as_str(), m.gap)).collect();
        assert_eq!(
            times,
            [
                ("09:12", false),
                ("09:13", false),
                ("09:40", true),
                ("09:41", false),
                ("10:30", true)
            ]
        );
        assert!(!acme.signals.iter().find(|s| s.title == "#inc-2291").unwrap().split);
        let chan = acme.signals.iter().find(|s| s.title == "#acme-incident").unwrap();
        assert_eq!(
            chan.messages[0].replies.len(),
            4,
            "the thread rides with its first message"
        );
    }

    #[test]
    fn split_review_moves_dismisses_and_confirms_parts() {
        let (conn, now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let q4 = topic_named(&conn, "Q4 headcount");
        let dm: i64 = conn
            .query_row(
                "SELECT id FROM items WHERE title = 'DM · Jordan' AND parent_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            None,
            &settings,
        )
        .unwrap();
        assert_eq!((f.conv, f.root, f.messages.len()), ("dm", dm, 11));
        let topics: Vec<Option<i64>> = f.parts.iter().map(|p| p.topic.as_ref().map(|t| t.id)).collect();
        assert_eq!(topics, [Some(acme), Some(q4), None]);
        let uns = f.parts[2].item_id;
        let at = |t: &str| f.messages.iter().find(|m| m.time == t).unwrap().id;
        assert!(f.messages.iter().all(|m| m.part.is_some()));

        // t to a topic that has a group: straight into that part; u moves it back.
        assert_eq!(
            review_move(&crate::store::over(&conn), uns, &[at("10:04")], To::Topic(acme), None).unwrap(),
            dm
        );
        assert_eq!(
            family(
                &crate::store::over(&conn),
                &crate::sync::test_clock("2026-09-27T10:52:00Z"),
                dm,
                None,
                &settings
            )
            .unwrap()
            .messages
            .iter()
            .find(|m| m.time == "10:04")
            .unwrap()
            .part,
            Some(dm)
        );
        review_move(&crate::store::over(&conn), dm, &[at("10:04")], To::Part(uns), None).unwrap();

        // A topic with no group yet: a new part, suggested to it.
        let career = topic_named(&conn, "Career");
        let part = review_move(&crate::store::over(&conn), uns, &[at("10:02")], To::Topic(career), None).unwrap();
        assert!(part != uns && part != dm);
        assert_eq!(link_state(&conn, part, career).unwrap().as_deref(), Some("suggested"));
        // d: dismissed in place, into the family's seen part; a second one joins it.
        let bin = review_dismiss(&crate::store::over(&conn), uns, &[at("10:03")]).unwrap();
        assert_eq!(
            review_dismiss(&crate::store::over(&conn), uns, &[at("10:04")]).unwrap(),
            bin
        );
        let f2 = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            None,
            &settings,
        )
        .unwrap();
        assert!(f2.parts.iter().any(|p| p.item_id == bin && p.dismissed));
        assert_eq!(
            f2.messages.len(),
            11,
            "dismissed messages stay in the review, struck through"
        );
        assert!(!exists(&conn, uns), "the emptied Unsorted part is pruned");
        // u: back to Unsorted, which is made again.
        let uns2 = review_move(&crate::store::over(&conn), bin, &[at("10:04")], To::Unsorted, None).unwrap();
        assert_eq!(
            review_move(&crate::store::over(&conn), bin, &[at("10:03")], To::Unsorted, None).unwrap(),
            uns2
        );
        assert!(!exists(&conn, bin), "the dismissed part goes once it's empty");
        let seen: bool = conn
            .query_row("SELECT seen FROM items WHERE id = ?1", [uns2], |r| r.get(0))
            .unwrap();
        assert!(!seen);
        // Moving every message out of a part removes it.
        let career_msgs = super::super::split::message_ids(&conn, part).unwrap();
        review_move(&crate::store::over(&conn), part, &career_msgs, To::Unsorted, None).unwrap();
        assert!(!exists(&conn, part));
        assert_eq!(super::super::split::message_ids(&conn, uns2).unwrap().len(), 3);

        // ↵ confirms every part with a topic; Unsorted stays.
        assert_eq!(
            review_confirm(
                &crate::store::over(&conn),
                crate::time::Ts::new(jiff::Timestamp::now()),
                dm
            )
            .unwrap(),
            2
        );
        assert_eq!(link_state(&conn, dm, acme).unwrap().as_deref(), Some("confirmed"));
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(!q
            .topics
            .iter()
            .flat_map(|t| &t.signals)
            .any(|s| s.title == "DM · Jordan"));
        assert!(q.unsorted.iter().any(|s| s.item_id == uns2));
        assert_eq!(unseen(&conn) as usize, shown(&q));
        // Moving Unsorted's messages into a confirmed part files them there:
        // the family is settled and the conversation is marked read once.
        let rest = super::super::split::message_ids(&conn, uns2).unwrap();
        review_move(&crate::store::over(&conn), uns2, &rest, To::Part(dm), None).unwrap();
        let marks: i64 = conn
            .query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(marks, 0, "a move isn't a confirm");
    }

    #[test]
    fn reclassify_moves_a_filed_signal_and_undo_puts_every_link_back() {
        let (conn, _now, _d) = setup();
        let acme = topic_named(&conn, "Acme outage");
        let webhook = topic_named(&conn, "Webhook reliability");
        let item: i64 = conn
            .query_row(
                "SELECT item_id FROM topic_links WHERE topic_id = ?1 AND state = 'confirmed' LIMIT 1",
                [acme],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'wrong')",
            params![item, webhook],
        )
        .unwrap();
        let moved = reclassify(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            item,
            Some(acme),
            webhook,
        )
        .unwrap();
        assert_eq!(
            moved,
            [Moved {
                item_id: item,
                from_state: Some("confirmed".into()),
                to_state: Some("wrong".into()),
                seen: true,
                unread: vec![]
            }]
        );
        assert_eq!(
            link_state(&conn, item, acme).unwrap().as_deref(),
            Some("wrong"),
            "one topic per signal"
        );
        assert_eq!(
            link_state(&conn, item, webhook).unwrap().as_deref(),
            Some("confirmed"),
            "filed straight away"
        );
        unreclassify(&crate::store::over(&conn), &moved, Some(acme), webhook).unwrap();
        assert_eq!(link_state(&conn, item, acme).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, item, webhook).unwrap().as_deref(), Some("wrong"));
        // A signal with no topic (from search) just gets one; undo removes it.
        let pr: i64 = conn
            .query_row("SELECT id FROM items WHERE kind = 'pr' AND seen = 0 LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let moved = reclassify(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            pr,
            None,
            webhook,
        )
        .unwrap();
        assert_eq!(link_state(&conn, pr, webhook).unwrap().as_deref(), Some("confirmed"));
        unreclassify(&crate::store::over(&conn), &moved, None, webhook).unwrap();
        assert_eq!(link_state(&conn, pr, webhook).unwrap(), None);
        let seen: bool = conn
            .query_row("SELECT seen FROM items WHERE id = ?1", [pr], |r| r.get(0))
            .unwrap();
        assert!(!seen, "back in Incoming as it was");
    }

    #[test]
    fn unfile_puts_back_a_confirm_a_move_and_a_wrong() {
        let (conn, _now, _d) = setup();
        let now = || crate::time::Ts::new(jiff::Timestamp::now());
        let uow = crate::store::over(&conn);
        let acme = topic_named(&conn, "Acme outage");
        let webhook = topic_named(&conn, "Webhook reliability");
        let pr: i64 = conn
            .query_row("SELECT id FROM items WHERE kind = 'pr' AND seen = 0 LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let seen = || -> bool {
            conn.query_row("SELECT seen FROM items WHERE id = ?1", [pr], |r| r.get(0))
                .unwrap()
        };
        conn.execute("DELETE FROM topic_links WHERE item_id = ?1", [pr])
            .unwrap();
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
            params![pr, acme],
        )
        .unwrap();
        let before = (
            link_state(&conn, pr, acme).unwrap(),
            link_state(&conn, pr, webhook).unwrap(),
        );
        let moved = snapshot(&uow, pr, None, acme).unwrap();
        wrong(&uow, pr, acme).unwrap();
        assert_eq!(link_state(&conn, pr, acme).unwrap().as_deref(), Some("wrong"));
        unfile(&uow, &moved, None, acme, false).unwrap();
        assert_eq!(link_state(&conn, pr, acme).unwrap(), before.0);
        let moved = snapshot(&uow, pr, Some(acme), webhook).unwrap();
        move_to(&uow, now(), pr, webhook, Some(acme)).unwrap();
        assert_eq!(link_state(&conn, pr, webhook).unwrap().as_deref(), Some("confirmed"));
        assert!(seen());
        unfile(&uow, &moved, Some(acme), webhook, true).unwrap();
        assert_eq!(
            (
                link_state(&conn, pr, acme).unwrap(),
                link_state(&conn, pr, webhook).unwrap()
            ),
            before
        );
        assert!(!seen(), "back in Incoming");
    }

    #[test]
    fn reclassifying_a_gmail_thread_swaps_its_category_label() {
        let (conn, _now, _d) = setup();
        conn.execute("UPDATE sources SET config_json = '{\"category_labels\": {\"work\": \"Work\", \"jobs\": \"Jobs\"}}' WHERE kind = 'gmail'", []).unwrap();
        let career = topic_named(&conn, "Career");
        let acme = topic_named(&conn, "Acme outage");
        let mail: i64 = conn
            .query_row("SELECT id FROM items WHERE title LIKE 'Lee%calibration%'", [], |r| {
                r.get(0)
            })
            .unwrap();
        conn.execute(
            "UPDATE items SET raw_json = '{\"thread_id\": \"t1\"}' WHERE id = ?1",
            [mail],
        )
        .unwrap();
        confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            mail,
            career,
        )
        .unwrap();
        conn.execute("DELETE FROM outbox", []).unwrap();
        reclassify(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            mail,
            Some(career),
            acme,
        )
        .unwrap();
        let payload: String = conn
            .query_row("SELECT payload_json FROM outbox WHERE op = 'gmail.relabel'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&payload).unwrap(),
            json!({ "thread_id": "t1", "remove": "Jobs", "add": "Work" })
        );
    }

    #[test]
    fn a_filed_signals_review_files_moves_straight_away() {
        let (conn, _now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let webhook = topic_named(&conn, "Webhook reliability");
        let dm: i64 = conn
            .query_row(
                "SELECT id FROM items WHERE title = 'DM · Jordan' AND parent_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        review_confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            dm,
        )
        .unwrap();
        // Reviewing just this filed signal: only its own part (and nothing from Incoming).
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            Some(&[dm]),
            &settings,
        )
        .unwrap();
        assert_eq!(f.parts.len(), 1);
        assert_eq!(f.parts[0].topic.as_ref().map(|t| t.id), Some(acme));
        assert_eq!(f.messages.len(), 5);
        let last = f.messages.last().unwrap().id;
        let part = review_move(&crate::store::over(&conn), dm, &[last], To::Topic(webhook), Some(&[dm])).unwrap();
        assert_eq!(link_state(&conn, part, webhook).unwrap().as_deref(), Some("confirmed"));
        let seen: bool = conn
            .query_row("SELECT seen FROM items WHERE id = ?1", [part], |r| r.get(0))
            .unwrap();
        assert!(seen, "filed, not back in Incoming");
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            Some(&[dm, part]),
            &settings,
        )
        .unwrap();
        assert_eq!(f.parts.len(), 2);
        // u: back to its old group by topic.
        assert_eq!(
            review_move(
                &crate::store::over(&conn),
                part,
                &[last],
                To::Topic(acme),
                Some(&[dm, part])
            )
            .unwrap(),
            dm
        );
        assert!(!exists(&conn, part));
        let d = detail(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            &settings,
        )
        .unwrap();
        assert_eq!((d.messages.len(), d.source.as_str()), (5, "slack"));
    }

    #[test]
    fn an_unsplit_signal_can_be_split_in_the_review() {
        let (conn, now, _d) = setup();
        let settings = PeopleSettings::default();
        let acme = topic_named(&conn, "Acme outage");
        let q4 = topic_named(&conn, "Q4 headcount");
        // Under a topic: one part, its suggested topic.
        let inc: i64 = conn
            .query_row("SELECT id FROM items WHERE title = '#inc-2291'", [], |r| r.get(0))
            .unwrap();
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            inc,
            None,
            &settings,
        )
        .unwrap();
        assert_eq!(f.parts.len(), 1);
        assert_eq!(f.parts[0].topic.as_ref().map(|t| t.id), Some(acme));
        let marco = f.messages.iter().find(|m| m.author == "Marco R.").unwrap().id;
        let part = review_move(&crate::store::over(&conn), inc, &[marco], To::Topic(q4), None).unwrap();
        assert_ne!(part, inc);
        assert_eq!(link_state(&conn, part, q4).unwrap().as_deref(), Some("suggested"));
        assert_eq!(
            review_confirm(
                &crate::store::over(&conn),
                crate::time::Ts::new(jiff::Timestamp::now()),
                inc
            )
            .unwrap(),
            2
        );
        assert_eq!(link_state(&conn, inc, acme).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, part, q4).unwrap().as_deref(), Some("confirmed"));

        // In Unsorted: the rest stays there.
        let dm: i64 = conn
            .query_row("SELECT id FROM items WHERE title = 'DM · Priya'", [], |r| r.get(0))
            .unwrap();
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            dm,
            None,
            &settings,
        )
        .unwrap();
        assert!(f.parts[0].topic.is_none());
        let first = f.messages[0].id;
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        review_move(&crate::store::over(&conn), dm, &[first], To::Topic(v1), None).unwrap();
        assert_eq!(
            review_confirm(
                &crate::store::over(&conn),
                crate::time::Ts::new(jiff::Timestamp::now()),
                dm
            )
            .unwrap(),
            1
        );
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(q.unsorted.iter().any(|s| s.item_id == dm && s.messages.len() == 3));
        assert_eq!(unseen(&conn) as usize, shown(&q));
    }

    #[test]
    fn confirming_a_review_leaves_the_unsorted_group_in_incoming() {
        let (conn, now, _d) = setup();
        let chan: i64 = conn
            .query_row(
                "SELECT id FROM items WHERE title = '#acme-incident' AND parent_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let f = family(
            &crate::store::over(&conn),
            &crate::sync::test_clock("2026-09-27T10:52:00Z"),
            chan,
            None,
            &PeopleSettings::default(),
        )
        .unwrap();
        assert_eq!(f.conv, "channel");
        assert_eq!(f.messages[0].replies.len(), 4);
        let uns = f.parts.iter().find(|p| p.topic.is_none()).unwrap().item_id;
        assert_eq!(
            review_confirm(
                &crate::store::over(&conn),
                crate::time::Ts::new(jiff::Timestamp::now()),
                chan
            )
            .unwrap(),
            2
        );
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(q.unsorted.iter().any(|s| s.item_id == uns));
        let marks: i64 = conn
            .query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(marks, 0, "not marked read while part of it is still in Incoming");
        assert!(review_move(&crate::store::over(&conn), uns, &[9999], To::Part(chan), None).is_err());
    }

    #[test]
    fn split_from_unsorted_confirms_only_the_selection() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let dm = q.unsorted.iter().find(|s| s.title == "DM · Priya").unwrap();
        assert_eq!(dm.messages.len(), 4);
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        let picked = [dm.messages[1].id, dm.messages[0].id, dm.messages[0].id];
        let child = split_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            dm.item_id,
            &picked,
            v1,
        )
        .unwrap();
        assert_ne!(child, dm.item_id);
        assert_eq!(link_state(&conn, child, v1).unwrap().as_deref(), Some("confirmed"));
        assert_eq!(link_state(&conn, dm.item_id, v1).unwrap(), None);

        let q2 = queue(&crate::store::over(&conn), &now).unwrap();
        assert!(
            !q2.unsorted.iter().any(|s| s.item_id == child),
            "the split-off part is filed"
        );
        let rest = q2.unsorted.iter().find(|s| s.item_id == dm.item_id).unwrap();
        let texts: Vec<&str> = rest.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts[0].starts_with("Different thing"));
        assert_eq!(rest.meta, "2 new · 09:16–09:20");
        assert_eq!(unseen(&conn) as usize, shown(&q2));

        // The child shows in the topic's chronology like any confirmed item.
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM topic_links WHERE topic_id = ?1 AND item_id = ?2 AND state = 'confirmed'",
                params![v1, child],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);

        // Selecting everything that's left is a plain move of the item.
        let all: Vec<i64> = rest.messages.iter().map(|m| m.id).collect();
        let q4 = topic_named(&conn, "Q4 headcount");
        assert_eq!(
            split_to(
                &crate::store::over(&conn),
                crate::time::Ts::new(jiff::Timestamp::now()),
                dm.item_id,
                &all,
                q4
            )
            .unwrap(),
            dm.item_id
        );
        assert_eq!(link_state(&conn, dm.item_id, q4).unwrap().as_deref(), Some("confirmed"));
        // Nothing selected, or someone else's message: refused, nothing changes.
        assert!(split_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            dm.item_id,
            &[],
            q4
        )
        .is_err());
        let before: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        let other = q2.topics[0]
            .signals
            .iter()
            .find(|s| !s.messages.is_empty())
            .unwrap()
            .messages[0]
            .id;
        assert!(split_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            dm.item_id,
            &[all[0], other],
            q4
        )
        .is_err());
        let after: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
    }

    /// A run of DM messages in the sample Slack source, keyed by ts, suggested to `topic`.
    fn slack_run(conn: &Connection, ts: &[&str]) -> i64 {
        use crate::sync::{apply_now, Batch, ItemUpsert, Message as M};
        let sid: i64 = conn
            .query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0))
            .unwrap();
        let src = crate::sync::load_source(conn, sid).unwrap();
        let messages = ts
            .iter()
            .map(|t| M {
                at: crate::time::Ts::from_unix_secs(1_790_000_000.0 + t.parse::<f64>().unwrap()).unwrap(),
                author: "Ana".into(),
                text: format!("m {t}"),
                ..Default::default()
            })
            .collect();
        let it = ItemUpsert {
            external_id: "D9/100.1".into(),
            kind: "thread".into(),
            title: "DM · Ana".into(),
            occurred_at: crate::time::t("2026-09-27T09:00:00"),
            status: "open".into(),
            raw_json: json!({ "channel": "D9", "mark_ts": ts[ts.len() - 1], "msgs": ts }),
            messages,
            bump: true,
            ..Default::default()
        };
        apply_now(
            conn,
            &src,
            &Batch {
                items: vec![it],
                ..Default::default()
            },
        )
        .unwrap();
        conn.query_row("SELECT id FROM items WHERE external_id = 'D9/100.1'", [], |r| r.get(0))
            .unwrap()
    }

    fn listed(conn: &Connection, now: &Clock, item: i64) -> Saw {
        let q = queue(&crate::store::over(conn), now).unwrap();
        let s = q
            .topics
            .iter()
            .flat_map(|t| &t.signals)
            .chain(&q.unsorted)
            .find(|s| s.item_id == item)
            .unwrap();
        Saw {
            rev: s.rev,
            keys: s.keys.clone(),
        }
    }

    #[test]
    fn a_message_arriving_mid_render_stays_new() {
        let (conn, now, _d) = setup();
        let item = slack_run(&conn, &["100.1", "100.2"]);
        let topic = topic_named(&conn, "Q4 planning");
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
            params![item, topic],
        )
        .unwrap();
        let saw = listed(&conn, &now, item);
        assert_eq!(saw.keys, ["100.1", "100.2"]);
        // A sync lands a third message after the list was rendered.
        slack_run(&conn, &["100.1", "100.2", "100.3"]);
        let changed = confirm_seen(&crate::store::over(&conn), now.now(), item, topic, Some(&saw)).unwrap();
        assert!(changed);
        // Filed under the topic, still in Incoming, with only the new message new.
        assert_eq!(link_state(&conn, item, topic).unwrap().as_deref(), Some("confirmed"));
        let seen: bool = conn
            .query_row("SELECT seen FROM items WHERE id = ?1", [item], |r| r.get(0))
            .unwrap();
        assert!(!seen);
        let read: Vec<(String, bool)> = conn
            .prepare("SELECT ext_key, read FROM item_messages WHERE item_id = ?1 ORDER BY at")
            .unwrap()
            .query_map([item], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            read,
            vec![("100.1".into(), true), ("100.2".into(), true), ("100.3".into(), false)]
        );
        // Slack is marked read up to what you saw, not past it.
        let mark: String = conn
            .query_row(
                "SELECT payload_json FROM outbox WHERE op = 'slack.mark_read'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&mark).unwrap()["ts"], "100.2");
    }

    #[test]
    fn nothing_new_since_you_saw_it_leaves_incoming() {
        let (conn, now, _d) = setup();
        let item = slack_run(&conn, &["100.1", "100.2"]);
        let saw = listed(&conn, &now, item);
        assert!(!dismiss_seen(&crate::store::over(&conn), item, Some(&saw)).unwrap());
        let seen: bool = conn
            .query_row("SELECT seen FROM items WHERE id = ?1", [item], |r| r.get(0))
            .unwrap();
        assert!(seen);
    }

    #[test]
    fn gmail_files_and_archives_only_the_mail_you_saw() {
        let (conn, now, _d) = setup();
        let thread = q_gmail_thread(&conn);
        let topic = topic_named(&conn, "Acme outage");
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
            params![thread, topic],
        )
        .unwrap();
        // You saw g1; g2 came in after.
        let saw = Saw {
            rev: -1,
            keys: vec!["g1".into()],
        };
        confirm_seen(&crate::store::over(&conn), now.now(), thread, topic, Some(&saw)).unwrap();
        let payload = |op: &str| -> Value {
            let p: String = conn
                .query_row("SELECT payload_json FROM outbox WHERE op = ?1", [op], |r| r.get(0))
                .unwrap();
            serde_json::from_str(&p).unwrap()
        };
        assert_eq!(payload("gmail.file")["message_ids"], json!(["g1"]));
        dismiss_seen(&crate::store::over(&conn), thread, Some(&saw)).unwrap();
        assert_eq!(payload("gmail.archive")["message_ids"], json!(["g1"]));
    }

    /// A Gmail thread item of the sample Gmail source with messages g1 and g2.
    fn q_gmail_thread(conn: &Connection) -> i64 {
        let sid: i64 = conn
            .query_row("SELECT id FROM sources WHERE kind = 'gmail'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            r#"INSERT INTO items (source_id, external_id, kind, title, occurred_at, raw_json) VALUES (?1, 'th1', 'thread', 'Re: renewal', '2026-09-27T09:00:00.000Z', '{"thread_id":"th1","msgs":["g1","g2"]}')"#,
            [sid],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn a_chat_items_conversation_kind_is_its_conv_else_slacks_title() {
        assert_eq!(conv_kind("Chat · Priya", &json!({"channel": "c", "conv": "dm"})), "dm");
        assert_eq!(
            conv_kind("Design review", &json!({"channel": "c", "conv": "group"})),
            "group"
        );
        assert_eq!(
            conv_kind("x", &json!({"channel": "c", "conv": "dm", "thread_ts": "1"})),
            "thread"
        );
        // Slack's items say nothing: their titles do.
        assert_eq!(conv_kind("DM · Priya", &json!({"channel": "D1"})), "dm");
        assert_eq!(conv_kind("Group DM · a, b", &json!({"channel": "G1"})), "group");
        assert_eq!(conv_kind("#inc-2291", &json!({"channel": "C1"})), "channel");
    }

    #[test]
    fn split_family_marks_slack_read_once_all_parts_are_triaged() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
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
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
            params![child, q4],
        )
        .unwrap();
        // Confirming it while the older part is still new must not mark the DM
        // read in Slack (that would mark the older messages read too).
        confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            child,
            q4,
        )
        .unwrap();
        assert!(marks().is_empty());
        // Once the rest is triaged, the conversation is marked up to the newest part.
        let v1 = topic_named(&conn, "v1 ingest deprecation");
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
            params![dm.item_id, v1],
        )
        .unwrap();
        confirm(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            dm.item_id,
            v1,
        )
        .unwrap();
        let m = marks();
        assert_eq!(m.len(), 1);
        let v: Value = serde_json::from_str(&m[0]).unwrap();
        assert_eq!(v, json!({ "channel": "D1", "ts": "100.4" }));
    }

    #[test]
    fn move_from_suggestion_marks_it_wrong() {
        let (conn, now, _d) = setup();
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let q4 = &q.topics[topic(&q, "Q4 headcount").unwrap()];
        let item = q4.signals[0].item_id;
        let target: i64 = conn
            .query_row("SELECT id FROM topics WHERE name = 'Q4 planning'", [], |r| r.get(0))
            .unwrap();
        move_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            item,
            target,
            Some(q4.id),
        )
        .unwrap();
        assert_eq!(link_state(&conn, item, q4.id).unwrap().as_deref(), Some("wrong"));
        assert_eq!(link_state(&conn, item, target).unwrap().as_deref(), Some("confirmed"));
        // Picking a topic that was once marked wrong turns it back to confirmed.
        let q = queue(&crate::store::over(&conn), &now).unwrap();
        let acme = &q.topics[topic(&q, "Acme outage").unwrap()];
        let it = acme.signals[0].item_id;
        wrong(&crate::store::over(&conn), it, acme.id).unwrap();
        move_to(
            &crate::store::over(&conn),
            crate::time::Ts::new(jiff::Timestamp::now()),
            it,
            acme.id,
            None,
        )
        .unwrap();
        assert_eq!(link_state(&conn, it, acme.id).unwrap().as_deref(), Some("confirmed"));
    }
}

//! Answering from Alethiometer: calendar invites (RSVP, 14a/14b) and Slack
//! replies. Answering and triage are separate: an answer never confirms,
//! moves or dismisses a signal, and triage never answers.
//!
//! - `respond.rsvp {item_id, response: accepted|declined, comment?}` queues
//!   `gcal.rsvp` for the invite's event, or its whole series when it's
//!   recurring (user, 2026-10-07). `comment` only goes with a decline.
//! - `respond.slack {item_id, text, thread, message_key?}` queues `slack.reply`.
//!   With `thread` it goes in the item's thread (a thread item), else in the
//!   thread of `message_key` (a top-level message of the item), else of its
//!   newest top-level message; without, in the conversation itself.
//! - What a row shows of an answer is its latest outbox row (`Answer`), so
//!   it reads "sending" until the outbox confirms it. A failed answer blocks
//!   the item's later writes in the outbox, so answering again replaces it.
//! - An invite shows Accept/Decline while your answer in Google is
//!   `needsAction` and it hasn't ended; tentative ones get nothing (user,
//!   2026-10-07). After an answer from here it shows that answer.

use super::{arg_i64, arg_str};
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::domain::respond::AnswerRow;
use crate::error::{msg, Result};
use crate::outbox::request;
use crate::time::Ts;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// An invite's RSVP line.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Rsvp {
    /// Your answer in Google as last synced: needs_action | accepted | declined | tentative.
    pub status: String,
    /// The answer sent from here, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// Other blocks at the same time: "DD prep 16:45–17:15", "Life 17:30–18:30".
    pub overlaps: Vec<String>,
    /// The event in Google Calendar, where an answer can be changed.
    pub url: Option<String>,
}

/// An answer queued from here, as its outbox row stands.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Answer {
    pub outbox_id: i64,
    /// pending | sent | confirmed | failed
    pub state: String,
    /// accepted | declined, for an RSVP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    /// What a reply said, so a failed one can be edited and sent again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// When it was confirmed (queued, until then): "10:58", "Thu 14:00".
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "rsvp" => Route::Write(|uow, _, args| {
            let comment = args.get("comment").and_then(Value::as_str);
            let id = rsvp(uow, arg_i64(args, "item_id")?, &arg_str(args, "response")?, comment)?;
            Ok(json!({ "ok": true, "outbox_id": id }))
        }),
        "slack" => Route::Write(|uow, _, args| {
            let thread = args.get("thread").and_then(Value::as_bool).unwrap_or(false);
            let key = args.get("message_key").and_then(Value::as_str);
            let id = slack_reply(uow, arg_i64(args, "item_id")?, &arg_str(args, "text")?, thread, key)?;
            Ok(json!({ "ok": true, "outbox_id": id }))
        }),
        _ => return None,
    })
}

fn answer(row: &AnswerRow, clock: &Clock) -> Answer {
    let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or_default();
    let s = |k: &str| payload.get(k).and_then(Value::as_str).map(str::to_owned);
    Answer {
        outbox_id: row.outbox_id,
        state: row.state.clone(),
        response: s("response"),
        text: s("text"),
        at: super::incoming::time_label(row.confirmed_at.unwrap_or(row.created_at), clock),
        error: row.error.clone(),
    }
}

/// The RSVP line of each of these items that's an invite to show one for.
pub fn rsvps(r: &dyn Reads, item_ids: &[i64], clock: &Clock) -> Result<HashMap<i64, Rsvp>> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let invites = r.respond().invites(item_ids)?;
    if invites.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<i64> = invites.iter().map(|i| i.item_id).collect();
    let answers: HashMap<i64, AnswerRow> = r
        .respond()
        .latest_rsvps(&ids)?
        .into_iter()
        .map(|a| (a.item_id, a))
        .collect();
    let mut overlaps: HashMap<i64, Vec<String>> = HashMap::new();
    for o in r.respond().overlaps(&ids)? {
        let hm = |t: Ts| clock.local(t).strftime("%H:%M").to_string();
        // Life blocks never show a title (boundary rule).
        let what = o.title.unwrap_or_else(|| "Life".into());
        overlaps
            .entry(o.item_id)
            .or_default()
            .push(format!("{what} {}–{}", hm(o.start), hm(o.end)));
    }
    let now = clock.now();
    let mut out = HashMap::new();
    for inv in invites {
        let answer = answers.get(&inv.item_id).map(|a| answer(a, clock));
        let open = inv.response == "needsAction" && inv.end.unwrap_or(inv.start) > now;
        if !open && answer.is_none() {
            continue;
        }
        let status = if inv.response == "needsAction" {
            "needs_action".to_string()
        } else {
            inv.response
        };
        out.insert(
            inv.item_id,
            Rsvp {
                status,
                answer,
                overlaps: overlaps.remove(&inv.item_id).unwrap_or_default(),
                url: inv.url,
            },
        );
    }
    Ok(out)
}

/// The latest reply queued from each of these items.
pub fn replies(r: &dyn Reads, item_ids: &[i64], clock: &Clock) -> Result<HashMap<i64, Answer>> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(r.respond()
        .latest_replies(item_ids)?
        .iter()
        .map(|a| (a.item_id, answer(a, clock)))
        .collect())
}

/// Queue an answer, or put it in place of the failed one before it (which
/// would otherwise hold back every later write for the item).
/// `one_at_a_time`: refuse while the last one is still sending (an RSVP; replies queue up).
fn queue(
    uow: &dyn UnitOfWork,
    last: Option<&AnswerRow>,
    one_at_a_time: bool,
    source_id: i64,
    op: &str,
    payload: &Value,
    item_id: i64,
) -> Result<i64> {
    match last {
        Some(a) if one_at_a_time && (a.state == "pending" || a.state == "sent") => {
            Err(msg("your last answer is still sending"))
        }
        Some(a) if a.state == "failed" && uow.outbox().resend(a.outbox_id, &payload.to_string())? => Ok(a.outbox_id),
        _ => request(uow, source_id, op, payload, Some(item_id)),
    }
}

fn rsvp(uow: &dyn UnitOfWork, item_id: i64, response: &str, comment: Option<&str>) -> Result<i64> {
    if !matches!(response, "accepted" | "declined") {
        return Err(msg("an answer is accepted or declined"));
    }
    let comment = comment.map(str::trim).filter(|c| !c.is_empty());
    if comment.is_some() && response != "declined" {
        return Err(msg("a note only goes with a decline"));
    }
    let r = uow.reads();
    let invite = r
        .respond()
        .invites(&[item_id])?
        .pop()
        .ok_or_else(|| msg("not an invite you're a guest of"))?;
    let item = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?;
    let mut payload = json!({ "event_id": invite.event_id, "response": response });
    if let Some(c) = comment {
        payload["comment"] = json!(c);
    }
    let last = r.respond().latest_rsvps(&[item_id])?.pop();
    queue(uow, last.as_ref(), true, item.source_id, "gcal.rsvp", &payload, item_id)
}

fn slack_reply(uow: &dyn UnitOfWork, item_id: i64, text: &str, thread: bool, message_key: Option<&str>) -> Result<i64> {
    if text.trim().is_empty() {
        return Err(msg("nothing to send"));
    }
    let r = uow.reads();
    let item = uow.items().get(item_id)?.ok_or_else(|| msg("no such item"))?;
    if item.source_kind != "slack" {
        return Err(msg("only Slack signals take a reply"));
    }
    let raw: Value = serde_json::from_str(&item.raw_json).unwrap_or_default();
    let field = |k: &str| {
        raw.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let channel = field("channel").ok_or_else(|| msg("this signal doesn't say which conversation it's from"))?;
    let mut payload = json!({ "channel": channel, "text": text });
    if thread {
        let ts = match field("thread_ts") {
            Some(ts) => ts,
            None => {
                // Top-level messages of the item, oldest first.
                let tops: Vec<String> = r
                    .incoming()
                    .message_heads(&[item_id])?
                    .into_iter()
                    .filter(|m| !m.context && m.reply_to.is_none())
                    .filter_map(|m| m.ext_key)
                    .collect();
                match message_key {
                    Some(k) if tops.iter().any(|t| t == k) => k.to_owned(),
                    Some(_) => return Err(msg("that message isn't part of this signal")),
                    None => tops.last().cloned().ok_or_else(|| msg("no message to reply to"))?,
                }
            }
        };
        payload["thread_ts"] = json!(ts);
    }
    let last = r.respond().latest_replies(&[item_id])?.pop();
    queue(
        uow,
        last.as_ref(),
        false,
        item.source_id,
        "slack.reply",
        &payload,
        item_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries::incoming::{queue as incoming_queue, Signal};
    use rusqlite::Connection;

    fn setup() -> (Connection, Clock, tempfile::TempDir) {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let now = crate::sync::test_clock("2026-09-27T10:52:00Z");
        crate::seed::seed(&crate::store::over(&conn), &now, dir.path()).unwrap();
        (conn, now, dir)
    }

    fn item(conn: &Connection, title: &str) -> i64 {
        conn.query_row("SELECT id FROM items WHERE title = ?1", [title], |r| r.get(0))
            .unwrap()
    }

    fn signal(conn: &Connection, clock: &Clock, item_id: i64) -> Signal {
        let q = incoming_queue(&crate::store::over(conn), clock).unwrap();
        q.topics
            .into_iter()
            .flat_map(|t| t.signals)
            .chain(q.unsorted)
            .find(|s| s.item_id == item_id)
            .expect("in Incoming")
    }

    fn outbox(conn: &Connection, op: &str) -> Vec<(i64, String, String, Option<i64>)> {
        conn.prepare("SELECT id, state, payload_json, item_id FROM outbox WHERE op = ?1 ORDER BY id")
            .unwrap()
            .query_map([op], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn set_state(conn: &Connection, id: i64, state: &str) {
        conn.execute("UPDATE outbox SET state = ?2, confirmed_at = CASE WHEN ?2 = 'confirmed' THEN '2026-09-27T10:58:00Z' END, error = CASE WHEN ?2 = 'failed' THEN 'boom' END WHERE id = ?1", rusqlite::params![id, state]).unwrap();
    }

    fn uow(conn: &Connection) -> crate::store::Sql<'_> {
        crate::store::over(conn)
    }

    /// Answering never triages: the invite stays where it was in Incoming,
    /// and its row shows the answer from "sending" to "done".
    #[test]
    fn an_rsvp_goes_through_the_outbox_and_leaves_the_signal_alone() {
        let (conn, clock, _d) = setup();
        let invite = item(&conn, "Acme postmortem review");
        let before = signal(&conn, &clock, invite);
        let ext: String = conn
            .query_row("SELECT external_id FROM items WHERE id = ?1", [invite], |r| r.get(0))
            .unwrap();

        rsvp(&uow(&conn), invite, "accepted", None).unwrap();
        let rows = outbox(&conn, "gcal.rsvp");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(&rows[0].2).unwrap(),
            json!({ "event_id": ext, "response": "accepted" })
        );
        assert_eq!(rows[0].3, Some(invite));

        let after = signal(&conn, &clock, invite);
        assert_eq!(
            (after.update, after.rev),
            (before.update, before.rev),
            "still the same untriaged signal"
        );
        let answer = after.rsvp.unwrap().answer.unwrap();
        assert_eq!(
            (answer.state.as_str(), answer.response.as_deref()),
            ("pending", Some("accepted"))
        );

        // One answer at a time.
        assert!(rsvp(&uow(&conn), invite, "declined", None)
            .unwrap_err()
            .to_string()
            .contains("still sending"));

        set_state(&conn, rows[0].0, "confirmed");
        let answer = signal(&conn, &clock, invite).rsvp.unwrap().answer.unwrap();
        assert_eq!((answer.state.as_str(), answer.at.as_str()), ("confirmed", "10:58"));
    }

    #[test]
    fn a_note_goes_only_with_a_decline() {
        let (conn, _clock, _d) = setup();
        let invite = item(&conn, "Acme postmortem review");
        assert!(rsvp(&uow(&conn), invite, "accepted", Some("see you")).is_err());
        assert!(
            rsvp(&uow(&conn), invite, "tentative", None).is_err(),
            "only accept and decline are offered"
        );
        rsvp(&uow(&conn), invite, "declined", Some("  Kim has signing authority ")).unwrap();
        let p: Value = serde_json::from_str(&outbox(&conn, "gcal.rsvp")[0].2).unwrap();
        assert_eq!(p["comment"], "Kim has signing authority");
    }

    /// A failed answer would hold back the item's later writes, so answering
    /// again goes out in its place.
    #[test]
    fn answering_again_replaces_a_failed_answer() {
        let (conn, clock, _d) = setup();
        let invite = item(&conn, "Acme postmortem review");
        rsvp(&uow(&conn), invite, "accepted", None).unwrap();
        let id = outbox(&conn, "gcal.rsvp")[0].0;
        set_state(&conn, id, "failed");
        let answer = signal(&conn, &clock, invite).rsvp.unwrap().answer.unwrap();
        assert_eq!(
            (answer.state.as_str(), answer.error.as_deref()),
            ("failed", Some("boom"))
        );

        assert_eq!(rsvp(&uow(&conn), invite, "declined", None).unwrap(), id);
        let rows = outbox(&conn, "gcal.rsvp");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "pending");
        assert!(rows[0].2.contains("declined"));
    }

    /// A recurring invite answers its series, and every instance shows it (user, 2026-10-07).
    #[test]
    fn a_recurring_invite_answers_the_series() {
        let (conn, clock, _d) = setup();
        let invite = item(&conn, "Acme postmortem review");
        let terms = item(&conn, "Final terms call");
        for id in [invite, terms] {
            conn.execute(
                "UPDATE items SET raw_json = json_set(raw_json, '$.recurringEventId', 'series1') WHERE id = ?1",
                [id],
            )
            .unwrap();
        }
        rsvp(&uow(&conn), invite, "declined", None).unwrap();
        let p: Value = serde_json::from_str(&outbox(&conn, "gcal.rsvp")[0].2).unwrap();
        assert_eq!(p["event_id"], "series1");
        let both = rsvps(&uow(&conn), &[invite, terms], &clock).unwrap();
        assert_eq!(
            both[&terms].answer.as_ref().unwrap().response.as_deref(),
            Some("declined")
        );
        assert_eq!(both[&invite].answer, both[&terms].answer);
    }

    /// Accept/Decline only for an unanswered invite that hasn't ended; an
    /// answer from here keeps showing whatever Google says since.
    #[test]
    fn which_invites_offer_an_answer() {
        let (conn, clock, _d) = setup();
        let terms = item(&conn, "Final terms call");
        let status = |s: &str| {
            conn.execute(
                "UPDATE items SET raw_json = json_set(raw_json, '$.attendees[0].responseStatus', ?2) WHERE id = ?1",
                rusqlite::params![terms, s],
            )
            .unwrap()
        };
        let shown = || rsvps(&uow(&conn), &[terms], &clock).unwrap().remove(&terms);

        assert_eq!(shown().unwrap().overlaps, ["Staff sync 14:00–14:30"]);
        status("tentative");
        assert!(shown().is_none(), "maybe is answered (user, 2026-10-07)");
        status("accepted");
        assert!(shown().is_none());
        status("needsAction");
        let ended = crate::sync::test_clock("2026-10-01T10:00:00Z");
        assert!(
            rsvps(&uow(&conn), &[terms], &ended).unwrap().is_empty(),
            "nothing to answer once it's over"
        );

        rsvp(&uow(&conn), terms, "accepted", None).unwrap();
        status("accepted");
        let s = shown().unwrap();
        assert_eq!(s.status, "accepted");
        assert!(s.answer.is_some());

        // Not a guest (your own event, no attendees): never an invite.
        let coffee = item(&conn, "Coffee");
        assert!(rsvp(&uow(&conn), coffee, "accepted", None).is_err());
    }

    #[test]
    fn a_slack_reply_goes_to_the_conversation_or_a_thread() {
        let (conn, clock, _d) = setup();
        let inc = item(&conn, "#inc-2291");
        let sent = |thread: bool, key: Option<&str>| -> Result<Value> {
            slack_reply(&uow(&conn), inc, "on it", thread, key)?;
            Ok(serde_json::from_str(&outbox(&conn, "slack.reply").last().unwrap().2).unwrap())
        };
        assert_eq!(
            sent(false, None).unwrap(),
            json!({ "channel": "C2291", "text": "on it" })
        );
        // Several replies queue up behind each other.
        assert_eq!(sent(true, None).unwrap()["thread_ts"], "C2291.3", "the newest message");
        assert_eq!(sent(true, Some("C2291.1")).unwrap()["thread_ts"], "C2291.1");
        assert!(sent(true, Some("C9.9")).is_err(), "a message of another signal");
        assert!(slack_reply(&uow(&conn), inc, "  ", false, None).is_err());

        // A thread item replies in its thread, whatever message is picked.
        conn.execute(
            "UPDATE items SET raw_json = json_set(raw_json, '$.thread_ts', '1.0') WHERE id = ?1",
            [inc],
        )
        .unwrap();
        assert_eq!(sent(true, Some("C2291.1")).unwrap()["thread_ts"], "1.0");

        let reply = signal(&conn, &clock, inc).reply.unwrap();
        assert_eq!(
            (reply.state.as_str(), reply.text.as_deref()),
            ("pending", Some("on it"))
        );

        let mail = item(&conn, "Dana · \"We need someone senior on a call today\"");
        assert!(slack_reply(&uow(&conn), mail, "hi", false, None).is_err());
    }
}

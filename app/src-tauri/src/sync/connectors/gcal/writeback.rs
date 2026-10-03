//! Writes to Google Calendar, only ever called by the outbox.
//!
//! - `gcal.rsvp {event_id, response}`: changes only your own attendee entry.
//! - `gcal.hold {item_id, title, carried_from, start?, end?}`: Reflect's "<
//!   schedule". Inserts a private, busy hold in the first free slot of the next
//!   workdays (or at `start`/`end` when given). The event id is derived from
//!   the payload, so a retry after a lost response finds the same event instead
//!   of creating a second one.

use super::{enc, rfc3339, Api, Config};
use crate::clock::Clock;
use crate::time::{Date, Ts};
use jiff::civil::Weekday;
use jiff::{SignedDuration, ToSpan};
use serde_json::{json, Value};

pub fn rsvp(api: &Api, cfg: &Config, payload: &Value) -> anyhow::Result<Value> {
    let event_id = payload.get("event_id").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("rsvp needs event_id"))?;
    let response = payload.get("response").and_then(Value::as_str).unwrap_or("");
    if !matches!(response, "accepted" | "declined" | "tentative") {
        anyhow::bail!("rsvp response must be accepted, declined or tentative");
    }
    let url = format!("{}/{}", cfg.events_url(), enc(event_id));
    let ev = api.get(&url)?.ok()?.json()?;
    let me = ev
        .get("attendees")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x.get("self").and_then(Value::as_bool) == Some(true)))
        .and_then(|x| x.get("email").and_then(Value::as_str))
        .ok_or_else(|| anyhow::anyhow!("you aren't on this event's guest list, so there's nothing to answer"))?
        .to_string();

    // `attendeesOmitted` tells Google this list holds only the entries to change,
    // so nobody else's entry is touched even if the event has hundreds of guests.
    let body = json!({ "attendeesOmitted": true, "attendees": [{ "email": me, "responseStatus": response }] });
    let patch_url = match &cfg.rsvp_send_updates {
        Some(s) => format!("{url}?sendUpdates={}", enc(s)),
        None => url,
    };
    let etag = ev.get("etag").and_then(Value::as_str).unwrap_or("").to_string();
    let extra: Vec<(&str, &str)> = if etag.is_empty() { vec![] } else { vec![("if-match", etag.as_str())] };
    let resp = api.request("PATCH", &patch_url, Some(&body), &extra)?;
    if resp.status == 412 {
        anyhow::bail!("the event changed in Google Calendar meanwhile; retry");
    }
    let updated = resp.ok()?.json()?;
    let now = updated
        .get("attendees")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x.get("email").and_then(Value::as_str) == Some(me.as_str())))
        .and_then(|x| x.get("responseStatus").and_then(Value::as_str));
    if let Some(now) = now {
        if now != response {
            anyhow::bail!("Google kept your response as '{now}'");
        }
    }
    Ok(json!({ "event_id": event_id, "response": response, "etag": updated.get("etag") }))
}

pub fn hold(api: &Api, cfg: &Config, payload: &Value) -> anyhow::Result<Value> {
    let title = payload.get("title").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).ok_or_else(|| anyhow::anyhow!("hold needs a title"))?;
    let item_id = payload.get("item_id").map(|v| v.to_string()).unwrap_or_default();
    let carried_from = payload.get("carried_from").and_then(Value::as_str).unwrap_or("");
    let id = hold_id(&cfg.calendar_id, &item_id, carried_from);

    let given = (
        payload.get("start").and_then(Value::as_str).and_then(Ts::parse),
        payload.get("end").and_then(Value::as_str).and_then(Ts::parse),
    );
    let (start, end) = match given {
        (Some(s), Some(e)) if s < e => (s, e),
        _ => {
            let today = api.ctx.clock.today();
            let from = carried_from.parse::<Date>().unwrap_or(today).max(today);
            find_slot(api, cfg, from)?
        }
    };

    let mut ev = json!({
        "id": id,
        "summary": title,
        "description": format!("Hold from Alethiometer (carried forward from {carried_from})."),
        "start": { "dateTime": rfc3339(start) },
        "end": { "dateTime": rfc3339(end) },
        "visibility": "private",
        "transparency": "opaque",
        "reminders": { "useDefault": true },
        "extendedProperties": { "private": { "alethiometer_item_id": item_id, "alethiometer_carried_from": carried_from } },
    });
    if let Some(c) = &cfg.hold_color {
        ev["colorId"] = json!(c);
    }
    let resp = api.request("POST", &cfg.events_url(), Some(&ev), &[])?;
    let created = if resp.status == 409 {
        // Already created by an earlier attempt whose response got lost.
        let existing = api.get(&format!("{}/{}", cfg.events_url(), enc(&id)))?.ok()?.json()?;
        if existing.get("status").and_then(Value::as_str) == Some("cancelled") {
            anyhow::bail!("this hold was created before and then deleted in Google Calendar; not re-creating it");
        }
        existing
    } else {
        resp.ok()?.json()?
    };
    Ok(json!({
        "event_id": created.get("id").and_then(Value::as_str).unwrap_or(&id),
        "start": start,
        "end": end,
        "html_link": created.get("htmlLink"),
    }))
}

/// Deterministic, valid Google event id (base32hex alphabet: 0-9, a-v).
fn hold_id(calendar: &str, item_id: &str, carried_from: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, format!("{calendar}|{item_id}|{carried_from}").as_bytes());
    let hex: String = d.as_ref()[..12].iter().map(|b| format!("{b:02x}")).collect();
    format!("alethiometer{hex}")
}

/// Next workdays after `after`, as many as the config searches.
fn workdays(after: Date, n: usize) -> Vec<Date> {
    (1..)
        .map(|i: i64| after + i.days())
        .filter(|d| !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday))
        .take(n)
        .collect()
}

/// First gap of `hold_minutes` inside the hold hours (wall-clock hours here),
/// reading busy time from the calendar.
fn find_slot(api: &Api, cfg: &Config, after: Date) -> anyhow::Result<(Ts, Ts)> {
    let clock = &api.ctx.clock;
    let days = workdays(after, cfg.hold_search_days);
    let (first, last) = (days[0], *days.last().unwrap_or(&days[0]));
    let at = |d: Date, h: u32| hour_here(clock, d, h);
    let base = format!(
        "{}?singleEvents=true&orderBy=startTime&maxResults=250&timeMin={}&timeMax={}",
        cfg.events_url(),
        enc(&rfc3339(at(first, 0))),
        enc(&rfc3339(at(last, 24)))
    );
    let mut busy = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let url = match &page {
            Some(p) => format!("{base}&pageToken={}", enc(p)),
            None => base.clone(),
        };
        let v = api.get(&url)?.ok()?.json()?;
        for ev in v.get("items").and_then(Value::as_array).into_iter().flatten() {
            if let Some(b) = busy_span(ev) {
                busy.push(b);
            }
        }
        match v.get("nextPageToken").and_then(Value::as_str) {
            Some(p) => page = Some(p.to_string()),
            None => break,
        }
    }
    busy.sort();
    let len = SignedDuration::from_mins(cfg.hold_minutes);
    for d in days {
        let (open, close) = (at(d, cfg.hold_day_start), at(d, cfg.hold_day_end));
        let mut t = quarter_up(open);
        for (s, e) in busy.iter().filter(|(s, e)| *e > open && *s < close) {
            if t + len <= *s {
                break;
            }
            t = quarter_up(t.max(*e));
        }
        if t + len <= close {
            return Ok((t, t + len));
        }
    }
    anyhow::bail!("no free {}-minute slot in the next {} workdays", cfg.hold_minutes, cfg.hold_search_days)
}

/// Hour `h` (0–24) of a day here; 24 is the start of the next day.
fn hour_here(clock: &Clock, d: Date, h: u32) -> Ts {
    if h >= 24 {
        clock.day_range(d).1
    } else {
        clock.from_local(d.at(h as i8, 0, 0, 0))
    }
}

/// Round up to the next quarter hour, so holds look like a person placed them.
/// (Every zone's offset is a whole number of quarter hours, so UTC quarters are local ones.)
fn quarter_up(t: Ts) -> Ts {
    const Q: i64 = 15 * 60 * 1000;
    let ms = t.unix_ms();
    Ts::from_unix_ms(ms.div_euclid(Q) * Q + if ms.rem_euclid(Q) == 0 { 0 } else { Q }).expect("in range")
}

/// Busy span of a listed event: timed, not free, not cancelled, not declined by you.
fn busy_span(ev: &Value) -> Option<(Ts, Ts)> {
    if ev.get("status").and_then(Value::as_str) == Some("cancelled") || ev.get("transparency").and_then(Value::as_str) == Some("transparent") {
        return None;
    }
    let declined = ev
        .get("attendees")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x.get("self").and_then(Value::as_bool) == Some(true)))
        .and_then(|x| x.get("responseStatus").and_then(Value::as_str))
        == Some("declined");
    if declined {
        return None;
    }
    let s = Ts::parse(ev.get("start")?.get("dateTime")?.as_str()?)?;
    let e = Ts::parse(ev.get("end")?.get("dateTime")?.as_str()?)?;
    Some((s, e))
}

#[cfg(test)]
mod tests {
    use super::super::oauth::TokenCache;
    use super::super::tests::{clock, fixture, http, now, secrets, source};
    use super::*;
    use crate::sync::http::FakeHttp;
    use crate::sync::Ctx;

    fn send(http: &FakeHttp, config: Value, op: &str, payload: Value) -> anyhow::Result<Value> {
        let src = source(config, None);
        let secret = secrets;
        let ctx = Ctx { source: &src, http, secret: &secret, now: now(), clock: clock() };
        let api = Api { ctx: &ctx, cache: &TokenCache::new() };
        let cfg = Config::from(&src.config);
        match op {
            "rsvp" => rsvp(&api, &cfg, &payload),
            _ => hold(&api, &cfg, &payload),
        }
    }

    fn sent(http: &FakeHttp, method: &str) -> Vec<(String, Value)> {
        http.requests()
            .into_iter()
            .filter(|r| r.0 == method && !r.1.contains("oauth2.googleapis.com"))
            .map(|r| (r.1, r.2.map(|b| serde_json::from_str(&b).unwrap()).unwrap_or(Value::Null)))
            .collect()
    }

    #[test]
    fn rsvp_patches_only_your_own_entry() {
        let http = http();
        http.on("GET", "/events/5fh2acmeqbr", 200, fixture!("event_invite.json"));
        http.on("PATCH", "/events/5fh2acmeqbr", 200, fixture!("event_invite_accepted.json"));
        let receipt = send(&http, json!({}), "rsvp", json!({ "event_id": "5fh2acmeqbr", "response": "accepted" })).unwrap();
        assert_eq!(receipt["response"], "accepted");
        assert_eq!(receipt["event_id"], "5fh2acmeqbr");
        let patches = sent(&http, "PATCH");
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].0, "https://www.googleapis.com/calendar/v3/calendars/primary/events/5fh2acmeqbr");
        assert_eq!(
            patches[0].1,
            json!({ "attendeesOmitted": true, "attendees": [{ "email": "alex.morgan@acme.com", "responseStatus": "accepted" }] })
        );
    }

    #[test]
    fn rsvp_send_updates_and_validation() {
        let http = http();
        http.on("GET", "/events/5fh2acmeqbr", 200, fixture!("event_invite.json"));
        http.on("PATCH", "/events/5fh2acmeqbr", 200, fixture!("event_invite_accepted.json"));
        let cfg = json!({ "rsvp_send_updates": "all", "calendar_id": "alex.morgan@acme.com" });
        // The fixture says accepted, so asking for tentative must not be reported as done.
        let e = send(&http, cfg.clone(), "rsvp", json!({ "event_id": "5fh2acmeqbr", "response": "tentative" })).unwrap_err();
        assert!(e.to_string().contains("kept your response as 'accepted'"));
        assert!(sent(&http, "PATCH")[0].0.ends_with("/calendars/alex.morgan%40acme.com/events/5fh2acmeqbr?sendUpdates=all"));
        assert!(send(&http, cfg, "rsvp", json!({ "event_id": "x", "response": "maybe" })).is_err());
    }

    #[test]
    fn rsvp_without_self_attendee_fails() {
        let http = http();
        http.on("GET", "/events/2b9vlunch0928", 200, r#"{"id":"2b9vlunch0928","status":"confirmed","summary":"Lunch"}"#);
        assert!(send(&http, json!({}), "rsvp", json!({ "event_id": "2b9vlunch0928", "response": "declined" })).is_err());
        assert!(sent(&http, "PATCH").is_empty());
    }

    #[test]
    fn hold_goes_into_the_first_free_slot_tomorrow() {
        let http = http();
        // The connector tests' clock is in UTC, so wall-clock hours there are UTC hours.
        let busy = |id: &str, s: &str, e: &str, extra: Value| {
            let mut ev = json!({ "id": id, "status": "confirmed",
                "start": { "dateTime": format!("{s}Z") }, "end": { "dateTime": format!("{e}Z") } });
            ev.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            ev
        };
        let list = json!({ "kind": "calendar#events", "items": [
            busy("a1", "2026-09-29T09:00:00", "2026-09-29T10:00:00", json!({})),
            busy("a2", "2026-09-29T10:15:00", "2026-09-29T10:50:00", json!({})),
            busy("a3", "2026-09-29T10:30:00", "2026-09-29T10:55:00", json!({})),
            busy("free", "2026-09-29T11:00:00", "2026-09-29T12:00:00", json!({ "transparency": "transparent" })),
            busy("no", "2026-09-29T11:00:00", "2026-09-29T12:00:00",
                json!({ "attendees": [{ "email": "alex.morgan@acme.com", "self": true, "responseStatus": "declined" }] })),
        ]});
        http.on("GET", "orderBy=startTime", 200, list.to_string());
        http.on("POST", "/calendars/primary/events", 200, fixture!("hold_created.json"));
        let payload = json!({ "item_id": 42, "title": "Draft Q4 hiring plan", "carried_from": "2026-09-28" });
        let receipt = send(&http, json!({ "hold_color": "3" }), "hold", payload.clone()).unwrap();
        let posts = sent(&http, "POST");
        let (url, ev) = posts.iter().find(|(u, _)| u.contains("/events")).unwrap();
        assert_eq!(url, "https://www.googleapis.com/calendar/v3/calendars/primary/events");
        // Busy 09:00-10:00 and 10:15-10:55; the 15-minute gap is too short, 10:55 rounds to 11:00;
        // free and declined events don't count.
        let expect_start = "2026-09-29T11:00:00Z";
        let expect_end = "2026-09-29T11:30:00Z";
        assert_eq!(ev["start"]["dateTime"], expect_start);
        assert_eq!(ev["end"]["dateTime"], expect_end);
        assert_eq!(ev["summary"], "Draft Q4 hiring plan");
        assert_eq!(ev["visibility"], "private");
        assert_eq!(ev["transparency"], "opaque");
        assert_eq!(ev["colorId"], "3");
        assert_eq!(ev["extendedProperties"]["private"]["alethiometer_item_id"], "42");
        let id = ev["id"].as_str().unwrap();
        assert!(id.len() >= 5 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='v').contains(&c)));
        assert_eq!(receipt["event_id"], "alethiometer0c2d9a8b7e6f5a4b3c2d1e0f");
        assert_eq!(receipt["start"], "2026-09-29T11:00:00.000Z");
        // Same payload, same id: a retry can't create a duplicate.
        assert_eq!(id, hold_id("primary", "42", "2026-09-28"));
    }

    #[test]
    fn hold_retry_after_lost_response_reuses_the_event() {
        let http = http();
        http.on("GET", "orderBy=startTime", 200, r#"{"kind":"calendar#events","items":[]}"#);
        http.on("POST", "/calendars/primary/events", 409, r#"{"error":{"code":409,"message":"The requested identifier already exists."}}"#);
        http.on("GET", "/events/alethiometer", 200, fixture!("hold_created.json"));
        let r = send(&http, json!({}), "hold", json!({ "item_id": 42, "title": "Draft Q4 hiring plan", "carried_from": "2026-09-28" })).unwrap();
        assert_eq!(r["event_id"], "alethiometer0c2d9a8b7e6f5a4b3c2d1e0f");
        // Empty day: first slot at the start of hold hours.
        assert_eq!(r["start"], "2026-09-29T09:00:00.000Z");
    }

    #[test]
    fn hold_honors_explicit_times_and_skips_weekends() {
        let http = http();
        http.on("POST", "/calendars/primary/events", 200, fixture!("hold_created.json"));
        let r = send(
            &http,
            json!({}),
            "hold",
            json!({ "item_id": 1, "title": "x", "carried_from": "2026-09-28", "start": "2026-10-02T14:00:00Z", "end": "2026-10-02T15:00:00Z" }),
        )
        .unwrap();
        assert_eq!(r["start"], "2026-10-02T14:00:00.000Z");
        assert!(sent(&http, "GET").is_empty(), "no slot search when times are given");
        let fri = jiff::civil::date(2026, 10, 2);
        assert_eq!(workdays(fri, 2), [jiff::civil::date(2026, 10, 5), jiff::civil::date(2026, 10, 6)]);
    }
}

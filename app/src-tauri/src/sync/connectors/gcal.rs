//! Google Calendar (the work calendar), via Calendar API v3.
//!
//! Poll: `events.list` with `singleEvents=true` over a window (default 30 days
//! back, 60 ahead), then incremental with the returned `syncToken`. A 410 Gone
//! means the token expired: do a full window sync again. Once a day the poll
//! does a full window sync anyway, because an incremental sync never brings
//! recurring instances that slide into the window without being edited.
//! `showDeleted=true` on both, so cancellations are never missed.
//!
//! The connector can't read the database, so "did this event's time or guests
//! change?" is answered from a small fingerprint map kept in the cursor next to
//! the sync token (`{sync_token, full_on, seen: {id: "end|hash|n"}}`).
//!
//! Writes (`send`): `gcal.rsvp` and `gcal.hold`, see `writeback`.

pub mod oauth;
mod writeback;

pub use oauth::call as auth_call;

use crate::sync::http::Response;
use crate::sync::{local_ts, Batch, Connector, Ctx, EventFields, Handle, ItemUpsert};
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone};
use oauth::TokenCache;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const API: &str = "https://www.googleapis.com/calendar/v3";

pub struct GoogleCalendar;

impl Connector for GoogleCalendar {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        poll_with(ctx, &oauth::CACHE)
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        let api = Api { ctx, cache: &oauth::CACHE };
        let cfg = Config::from(&ctx.source.config);
        match op {
            "gcal.rsvp" => writeback::rsvp(&api, &cfg, payload),
            "gcal.hold" => writeback::hold(&api, &cfg, payload),
            _ => anyhow::bail!("Google Calendar doesn't support '{op}'"),
        }
    }
}

// ---------------------------------------------------------------- config

/// `config` of a `gcal` source in settings.json. Every field is optional.
pub struct Config {
    pub calendar_id: String,
    pub days_back: i64,
    pub days_ahead: i64,
    /// Google `colorId` -> settings category key (e.g. {"3": "self"}).
    pub category_by_color: BTreeMap<String, String>,
    pub default_category: String,
    /// Holds: length, working hours to place them in, how many workdays to search.
    pub hold_minutes: i64,
    pub hold_day_start: u32,
    pub hold_day_end: u32,
    pub hold_search_days: usize,
    pub hold_color: Option<String>,
    /// `sendUpdates` for RSVPs (all | externalOnly | none). Unset = API default.
    pub rsvp_send_updates: Option<String>,
}

impl Config {
    pub fn from(v: &Value) -> Config {
        let s = |k: &str, d: &str| v.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let n = |k: &str, d: i64| v.get(k).and_then(Value::as_i64).unwrap_or(d);
        let hours: Vec<u32> = v
            .get("hold_hours")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).map(|h| h as u32).collect())
            .unwrap_or_default();
        let (hold_day_start, hold_day_end) = match hours.as_slice() {
            [a, b] if a < b && *b <= 24 => (*a, *b),
            _ => (9, 17),
        };
        Config {
            calendar_id: s("calendar_id", "primary"),
            days_back: n("days_back", 30).max(0),
            days_ahead: n("days_ahead", 60).max(1),
            category_by_color: v
                .get("category_by_color")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, c)| Some((k.clone(), c.as_str()?.to_string()))).collect())
                .unwrap_or_default(),
            default_category: s("default_category", "work"),
            hold_minutes: n("hold_minutes", 30).clamp(5, 480),
            hold_day_start,
            hold_day_end,
            hold_search_days: n("hold_search_days", 5).clamp(1, 20) as usize,
            hold_color: v.get("hold_color").and_then(Value::as_str).map(str::to_owned),
            rsvp_send_updates: v.get("rsvp_send_updates").and_then(Value::as_str).map(str::to_owned),
        }
    }

    fn events_url(&self) -> String {
        format!("{API}/calendars/{}/events", enc(&self.calendar_id))
    }
}

// ---------------------------------------------------------------- authorized requests

/// Requests with a bearer token; one refresh-and-retry on 401.
pub struct Api<'c, 'a> {
    pub ctx: &'c Ctx<'a>,
    pub cache: &'c TokenCache,
}

impl Api<'_, '_> {
    pub fn request(&self, method: &str, url: &str, body: Option<&Value>, extra: &[(&str, &str)]) -> anyhow::Result<Response> {
        let body = body.map(Value::to_string);
        for attempt in 0..2 {
            let auth = format!("Bearer {}", oauth::access_token(self.ctx, self.cache)?);
            let mut headers = vec![("authorization", auth.as_str()), ("accept", "application/json")];
            if body.is_some() {
                headers.push(("content-type", "application/json"));
            }
            headers.extend_from_slice(extra);
            let resp = self.ctx.http.request(method, url, &headers, body.as_deref())?;
            if resp.status == 401 && attempt == 0 {
                oauth::invalidate(self.ctx, self.cache);
                continue;
            }
            return Ok(resp);
        }
        unreachable!("the loop returns on its second attempt")
    }

    pub fn get(&self, url: &str) -> anyhow::Result<Response> {
        self.request("GET", url, None, &[])
    }
}

// ---------------------------------------------------------------- cursor

#[derive(Default)]
struct Cursor {
    sync_token: Option<String>,
    /// Local date of the last full window sync.
    full_on: String,
    seen: BTreeMap<String, Seen>,
}

/// What the last poll saw of an event, to decide whether an update is new signal.
#[derive(Clone, PartialEq, Debug)]
struct Seen {
    end: String,
    hash: String,
    needs_action: bool,
}

impl Cursor {
    fn parse(s: Option<&str>) -> Cursor {
        let Some(v) = s.and_then(|s| serde_json::from_str::<Value>(s).ok()) else { return Cursor::default() };
        let seen = v
            .get("seen")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(id, e)| {
                        let mut p = e.as_str()?.split('|');
                        let (end, hash, flag) = (p.next()?, p.next()?, p.next()?);
                        Some((id.clone(), Seen { end: end.into(), hash: hash.into(), needs_action: flag == "n" }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Cursor {
            sync_token: v.get("sync_token").and_then(Value::as_str).map(str::to_owned),
            full_on: v.get("full_on").and_then(Value::as_str).unwrap_or("").to_string(),
            seen,
        }
    }

    fn to_json(&self) -> String {
        let seen: Map<String, Value> = self
            .seen
            .iter()
            .map(|(id, s)| (id.clone(), json!(format!("{}|{}|{}", s.end, s.hash, if s.needs_action { "n" } else { "a" }))))
            .collect();
        json!({ "sync_token": self.sync_token, "full_on": self.full_on, "seen": seen }).to_string()
    }
}

// ---------------------------------------------------------------- poll

fn poll_with(ctx: &Ctx, cache: &TokenCache) -> anyhow::Result<Batch> {
    let cfg = Config::from(&ctx.source.config);
    let api = Api { ctx, cache };
    let mut cur = Cursor::parse(ctx.source.cursor.as_deref());
    let today = ctx.now.date().format("%Y-%m-%d").to_string();
    let window_start = ctx.now - Duration::days(cfg.days_back);

    let incremental = cur.sync_token.clone().filter(|_| cur.full_on == today);
    let listed = match incremental {
        Some(token) => list(&api, &cfg, Some(&token), ctx.now)?,
        None => None,
    };
    let (events, token) = match listed {
        Some(l) => l,
        // No token, a new day, or 410 Gone: full window sync.
        None => {
            cur.full_on = today;
            list(&api, &cfg, None, ctx.now)?.ok_or_else(|| anyhow::anyhow!("Google Calendar returned 410 on a full sync"))?
        }
    };

    let floor = crate::seed::fmt(window_start);
    let ceiling = crate::seed::fmt(ctx.now + Duration::days(cfg.days_ahead));
    let now = crate::seed::fmt(ctx.now);
    let first = ctx.source.cursor.is_none();
    let mut batch = Batch::default();
    for ev in &events {
        let Some(id) = ev.get("id").and_then(Value::as_str) else { continue };
        if ev.get("status").and_then(Value::as_str) == Some("cancelled") {
            // Kept locally, marked done, off the strip (see Batch::cancelled).
            if cur.seen.remove(id).is_some() {
                batch.cancelled.push(id.to_string());
            }
            continue;
        }
        let Some((item, seen)) = map_event(ev, &cfg, cur.seen.get(id)) else { continue };
        if seen.end < floor || item.occurred_at > ceiling {
            continue;
        }
        let quiet = first && !(seen.needs_action && seen.end >= now);
        cur.seen.insert(id.to_string(), seen);
        // Holds Alethiometer created itself (Reflect "< schedule") aren't news.
        if quiet || ev.pointer("/extendedProperties/private/alethiometer_item_id").is_some() {
            batch.seen.push(id.to_string());
        }
        batch.items.push(item);
    }
    cur.seen.retain(|_, s| s.end >= floor);
    cur.sync_token = Some(token);
    batch.cursor = Some(cur.to_json());
    Ok(batch)
}

/// All pages of one `events.list`. `Ok(None)` means 410 Gone (sync token expired).
fn list(api: &Api, cfg: &Config, sync_token: Option<&str>, now: NaiveDateTime) -> anyhow::Result<Option<(Vec<Value>, String)>> {
    // The same query both times: a sync token only works with the parameters it came from.
    let mut base = format!("{}?singleEvents=true&showDeleted=true&maxResults=250", cfg.events_url());
    match sync_token {
        Some(t) => base.push_str(&format!("&syncToken={}", enc(t))),
        None => {
            let from = rfc3339_local(now - Duration::days(cfg.days_back));
            let to = rfc3339_local(now + Duration::days(cfg.days_ahead));
            base.push_str(&format!("&timeMin={}&timeMax={}", enc(&from), enc(&to)));
        }
    }
    let mut events = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..200 {
        let url = match &page {
            Some(p) => format!("{base}&pageToken={}", enc(p)),
            None => base.clone(),
        };
        let resp = api.get(&url)?;
        if resp.status == 410 && sync_token.is_some() {
            return Ok(None);
        }
        let v = resp.ok()?.json()?;
        if let Some(items) = v.get("items").and_then(Value::as_array) {
            events.extend(items.iter().cloned());
        }
        if let Some(next) = v.get("nextPageToken").and_then(Value::as_str) {
            page = Some(next.to_string());
            continue;
        }
        let token = v
            .get("nextSyncToken")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("events.list ended without nextSyncToken"))?;
        return Ok(Some((events, token.to_string())));
    }
    anyhow::bail!("events.list: too many pages")
}

// ---------------------------------------------------------------- mapping

/// One Google event -> an item (plus a strip block when it's timed and not declined).
fn map_event(ev: &Value, cfg: &Config, prev: Option<&Seen>) -> Option<(ItemUpsert, Seen)> {
    let id = ev.get("id")?.as_str()?;
    // Working-location entries ("Home", "Office") aren't meetings.
    if ev.get("eventType").and_then(Value::as_str) == Some("workingLocation") {
        return None;
    }
    let (start, start_all_day) = when(ev.get("start")?)?;
    let (end, _) = when(ev.get("end")?)?;
    let attendees: Vec<&Value> = ev.get("attendees").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let me = attendees.iter().find(|a| a.get("self").and_then(Value::as_bool) == Some(true));
    // No own attendee entry: your own event without guests, so it's accepted.
    let response = me.and_then(|a| a.get("responseStatus")).and_then(Value::as_str).unwrap_or("accepted");
    let needs_action = response == "needsAction";
    let declined = response == "declined";
    let tentative = needs_action || response == "tentative" || ev.get("status").and_then(Value::as_str) == Some("tentative");

    let others: Vec<&&Value> = attendees
        .iter()
        .filter(|a| a.get("self").and_then(Value::as_bool) != Some(true) && a.get("resource").and_then(Value::as_bool) != Some(true))
        .collect();
    let who = attendee_summary(&others);

    let mut emails: Vec<&str> = others.iter().filter_map(|a| a.get("email").and_then(Value::as_str)).collect();
    emails.sort_unstable();
    let status = ev.get("status").and_then(Value::as_str).unwrap_or("confirmed");
    let hash = format!("{:08x}", fnv(format!("{start}|{end}|{status}|{}", emails.join(",")).as_bytes()) as u32);
    let seen = Seen { end: end.clone(), hash, needs_action };
    let bump = match prev {
        None => true,
        Some(p) => !declined && (p.hash != seen.hash || (needs_action && !p.needs_action)),
    };

    let category = ev
        .get("colorId")
        .and_then(Value::as_str)
        .and_then(|c| cfg.category_by_color.get(c))
        .cloned()
        .unwrap_or_else(|| cfg.default_category.clone());
    let title = ev.get("summary").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("(No title)").to_string();

    let mut notes: Vec<String> = who.iter().cloned().collect();
    if start_all_day {
        notes.push("all day".into());
    }
    if needs_action {
        notes.push("needs your response".into());
    } else if declined {
        notes.push("declined".into());
    } else if tentative {
        notes.push("tentative".into());
    }

    let organizer = ev.get("organizer");
    let author = organizer
        .filter(|o| o.get("self").and_then(Value::as_bool) != Some(true))
        .and_then(|o| {
            let email = o.get("email")?.as_str()?;
            let name = o.get("displayName").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| name_from_email(email));
            Some(Handle { handle: email.to_string(), display_name: name })
        });

    // All-day and declined events stay as items but don't draw a block on the day strip.
    let on_strip = !start_all_day && !declined;
    let item = ItemUpsert {
        external_id: id.to_string(),
        kind: "event".into(),
        title,
        body: ev.get("description").and_then(Value::as_str).map(strip_html).unwrap_or_default(),
        summary: if notes.is_empty() { None } else { Some(notes.join(" · ")) },
        url: ev.get("htmlLink").and_then(Value::as_str).map(str::to_owned),
        author,
        occurred_at: start.clone(),
        ended_at: Some(end.clone()),
        due_at: None,
        status: if declined { "done".into() } else { "open".into() },
        raw_json: ev.clone(),
        event: on_strip.then_some(EventFields { start, end, category, tentative, attendees: who }),
        bump,
        drop_event: !on_strip,
        ..Default::default()
    };
    Some((item, seen))
}

/// Local naive timestamp and whether it's an all-day date.
fn when(t: &Value) -> Option<(String, bool)> {
    if let Some(dt) = t.get("dateTime").and_then(Value::as_str) {
        // Offsets are always present in API responses; tolerate a bare local time anyway.
        return local_ts(dt).or_else(|| NaiveDateTime::parse_from_str(dt, "%Y-%m-%dT%H:%M:%S").ok().map(crate::seed::fmt)).map(|s| (s, false));
    }
    let d = NaiveDate::parse_from_str(t.get("date")?.as_str()?, "%Y-%m-%d").ok()?;
    Some((format!("{d}T00:00:00"), true))
}

/// "Priya, Jordan, Lee +2" (first names, self and rooms excluded).
fn attendee_summary(others: &[&&Value]) -> Option<String> {
    if others.is_empty() {
        return None;
    }
    let names: Vec<String> = others
        .iter()
        .take(3)
        .map(|a| match a.get("displayName").and_then(Value::as_str) {
            Some(n) if !n.trim().is_empty() => n.split_whitespace().next().unwrap_or(n).to_string(),
            _ => name_from_email(a.get("email").and_then(Value::as_str).unwrap_or("?")),
        })
        .collect();
    let mut s = names.join(", ");
    if others.len() > 3 {
        s.push_str(&format!(" +{}", others.len() - 3));
    }
    Some(s)
}

/// "priya.shah@acme.com" -> "Priya".
pub(crate) fn name_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let first = local.split(['.', '_', '+', '-']).next().unwrap_or(local);
    let mut c = first.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => email.to_string(),
    }
}

/// Descriptions may be HTML; keep the text for search.
pub(crate) fn strip_html(s: &str) -> String {
    let re = regex::Regex::new(r"(?i)<br\s*/?>|</p>|<[^>]+>").expect("static regex");
    let text = re.replace_all(s, |c: &regex::Captures| {
        let tag = c[0].to_ascii_lowercase();
        if tag.starts_with("<br") || tag == "</p>" {
            "\n"
        } else {
            ""
        }
    });
    text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&quot;", "\"").trim().to_string()
}

// ---------------------------------------------------------------- helpers

/// RFC 3339 with the local offset, for timeMin/timeMax and new events.
pub fn rfc3339_local(t: NaiveDateTime) -> String {
    match chrono::Local.from_local_datetime(&t).earliest() {
        Some(l) => l.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        None => format!("{}Z", t.format("%Y-%m-%dT%H:%M:%S")),
    }
}

/// Percent-encode everything but RFC 3986 unreserved characters.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// FNV-1a: stable across builds, unlike std's hasher.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;
    use crate::sync::Source;

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/gcal/", $name))
        };
    }
    pub(crate) use fixture;

    pub fn source(config: Value, cursor: Option<String>) -> Source {
        Source { id: 1, kind: "gcal".into(), tag: "work".into(), name: "Work calendar".into(), config, cursor }
    }

    pub fn secrets(k: &str) -> Option<String> {
        match k {
            "gcal.client_id" | "t.client_id" => Some("cid.apps.googleusercontent.com".into()),
            "gcal.client_secret" | "t.client_secret" => Some("GOCSPX-secret".into()),
            "gcal.refresh_token" | "t.refresh_token" => Some("1//0gRefreshTokenExample".into()),
            _ => None,
        }
    }

    pub fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-09-28T10:52:00", "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    pub fn http() -> FakeHttp {
        let http = FakeHttp::default();
        http.on("POST", "oauth2.googleapis.com/token", 200, fixture!("token_refresh.json"));
        http
    }

    fn poll(http: &FakeHttp, config: Value, cursor: Option<String>) -> Batch {
        let src = source(config, cursor);
        let secret = secrets;
        let ctx = Ctx { source: &src, http, secret: &secret, now: now() };
        poll_with(&ctx, &TokenCache::new()).unwrap()
    }

    fn cursor_json(b: &Batch) -> Value {
        serde_json::from_str(b.cursor.as_deref().unwrap()).unwrap()
    }

    fn item<'a>(b: &'a Batch, id: &str) -> &'a ItemUpsert {
        b.items.iter().find(|i| i.external_id == id).unwrap_or_else(|| panic!("no item {id}"))
    }

    fn first_sync() -> (FakeHttp, Batch) {
        let http = http();
        http.on("GET", "/calendars/primary/events?", 200, fixture!("full_page1.json"));
        http.on("GET", "pageToken=CiAKGjBwMnAzcGFnZTI", 200, fixture!("full_page2.json"));
        let b = poll(&http, json!({ "category_by_color": { "3": "self" } }), None);
        (http, b)
    }

    #[test]
    fn first_sync_pages_through_the_window() {
        let (http, b) = first_sync();
        let gets: Vec<String> = http.requests().into_iter().filter(|r| r.0 == "GET").map(|r| r.1).collect();
        assert_eq!(gets.len(), 2);
        assert!(gets[0].starts_with("https://www.googleapis.com/calendar/v3/calendars/primary/events?singleEvents=true&showDeleted=true&maxResults=250&timeMin=2026-08-29T10%3A52%3A00"));
        assert!(gets[0].contains("&timeMax=2026-11-27T10%3A52%3A00"));
        assert!(!gets[0].contains("syncToken"));
        assert!(gets[1].ends_with("&pageToken=CiAKGjBwMnAzcGFnZTI"));
        // workingLocation skipped; the cancelled one was never seen, so nothing to cancel.
        let ids: Vec<&str> = b.items.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(ids, ["7kq3c1standup_20260928T150000Z", "2b9vlunch0928", "5fh2acmeqbr", "6t1allhands", "9zz1offsite", "3dq8declined"]);
        assert!(b.cancelled.is_empty());
        let c = cursor_json(&b);
        assert_eq!(c["sync_token"], "CPDAlvWDx70CEPDAlvWDx70CGAU=");
        assert_eq!(c["full_on"], "2026-09-28");
        assert_eq!(c["seen"].as_object().unwrap().len(), 6);

        let standup = item(&b, "7kq3c1standup_20260928T150000Z");
        let ev = standup.event.as_ref().unwrap();
        assert_eq!(standup.title, "Platform standup");
        assert_eq!(ev.start, local_ts("2026-09-28T15:00:00Z").unwrap());
        assert_eq!(ev.end, local_ts("2026-09-28T15:15:00Z").unwrap());
        assert_eq!(ev.attendees.as_deref(), Some("Priya, Jordan, Lee +2"));
        assert_eq!(ev.category, "work");
        assert!(!ev.tentative);
        assert_eq!(standup.author.as_ref().unwrap().handle, "priya.shah@acme.com");
        assert_eq!(standup.author.as_ref().unwrap().display_name, "Priya Shah");
        assert_eq!(standup.url.as_deref(), Some("https://www.google.com/calendar/event?eid=N2txM2Mx"));
        assert!(standup.bump);
    }

    #[test]
    fn color_maps_to_category_and_self_holds_keep_titles() {
        let (_, b) = first_sync();
        let lunch = item(&b, "2b9vlunch0928");
        assert_eq!(lunch.title, "Lunch");
        let ev = lunch.event.as_ref().unwrap();
        assert_eq!(ev.category, "self");
        assert_eq!(ev.attendees, None);
        assert!(lunch.author.is_none(), "own events have no author person");
        // Unmapped colorId falls back to the default.
        assert_eq!(item(&b, "6t1allhands").event.as_ref().unwrap().category, "work");
    }

    #[test]
    fn own_response_drives_tentative_and_strip() {
        let (_, b) = first_sync();
        let invite = item(&b, "5fh2acmeqbr");
        assert!(invite.event.as_ref().unwrap().tentative);
        assert_eq!(invite.summary.as_deref(), Some("Sam, Dana · needs your response"));
        let allhands = item(&b, "6t1allhands");
        assert!(allhands.event.as_ref().unwrap().tentative);
        assert!(allhands.summary.as_deref().unwrap().ends_with("· tentative"));
        // Declined: kept, done, off the strip.
        let declined = item(&b, "3dq8declined");
        assert!(declined.event.is_none() && declined.drop_event);
        assert_eq!(declined.status, "done");
        // All-day: kept, off the strip.
        let offsite = item(&b, "9zz1offsite");
        assert!(offsite.event.is_none() && offsite.drop_event);
        assert_eq!(offsite.occurred_at, "2026-10-01T00:00:00");
        assert_eq!(offsite.ended_at.as_deref(), Some("2026-10-03T00:00:00"));
    }

    #[test]
    fn first_sync_arrives_read_except_pending_invites() {
        let (_, b) = first_sync();
        assert_eq!(b.seen, ["7kq3c1standup_20260928T150000Z", "2b9vlunch0928", "6t1allhands", "9zz1offsite", "3dq8declined"]);
    }

    #[test]
    fn alethiometer_made_holds_arrive_already_seen() {
        let (_, first) = first_sync();
        let http = http();
        let mut page: Value = serde_json::from_str(fixture!("incremental.json")).unwrap();
        page["items"][2]["extendedProperties"] = json!({ "private": { "alethiometer_item_id": "42" } });
        http.on("GET", "syncToken=CPDAlvWDx70CEPDAlvWDx70CGAU%3D", 200, page.to_string());
        let b = poll(&http, json!({}), first.cursor.clone());
        assert_eq!(b.seen, ["8nw1newinvite"]);
        assert!(b.items.iter().any(|i| i.external_id == "8nw1newinvite"));
    }

    #[test]
    fn incremental_sync_bumps_only_real_changes_and_cancels() {
        let (_, first) = first_sync();
        let http = http();
        http.on("GET", "syncToken=CPDAlvWDx70CEPDAlvWDx70CGAU%3D", 200, fixture!("incremental.json"));
        let b = poll(&http, json!({ "category_by_color": { "3": "self" } }), first.cursor.clone());
        let gets: Vec<String> = http.requests().into_iter().filter(|r| r.0 == "GET").map(|r| r.1).collect();
        assert_eq!(gets.len(), 1);
        assert!(!gets[0].contains("timeMin"));
        // Moved: bump. Only the description changed: no bump. New invite: new item.
        assert!(item(&b, "5fh2acmeqbr").bump);
        assert!(!item(&b, "7kq3c1standup_20260928T150000Z").bump);
        assert!(item(&b, "8nw1newinvite").bump);
        assert_eq!(b.cancelled, ["2b9vlunch0928"]);
        let c = cursor_json(&b);
        assert_eq!(c["sync_token"], "CKjC0YiEx70CEKjC0YiEx70CGAU=");
        assert!(c["seen"].get("2b9vlunch0928").is_none());
        assert!(c["seen"].get("8nw1newinvite").is_some());
    }

    #[test]
    fn instances_past_the_horizon_are_skipped() {
        let (_, first) = first_sync();
        let http = http();
        let mut page: Value = serde_json::from_str(fixture!("incremental.json")).unwrap();
        page["items"][2]["start"] = json!({ "dateTime": "2051-12-22T10:00:00Z" });
        page["items"][2]["end"] = json!({ "dateTime": "2051-12-22T10:30:00Z" });
        http.on("GET", "syncToken=CPDAlvWDx70CEPDAlvWDx70CGAU%3D", 200, page.to_string());
        let b = poll(&http, json!({}), first.cursor.clone());
        assert!(!b.items.iter().any(|i| i.external_id == "8nw1newinvite"));
        assert!(cursor_json(&b)["seen"].get("8nw1newinvite").is_none());
    }

    #[test]
    fn gone_sync_token_triggers_full_resync() {
        let (_, first) = first_sync();
        let mut cur = cursor_json(&first);
        cur["sync_token"] = json!("expired-token");
        let http = http();
        http.on("GET", "/calendars/primary/events?", 200, fixture!("full_page2.json"));
        http.on("GET", "syncToken=expired-token", 410, fixture!("gone.json"));
        let b = poll(&http, json!({}), Some(cur.to_string()));
        let gets: Vec<String> = http.requests().into_iter().filter(|r| r.0 == "GET").map(|r| r.1).collect();
        assert_eq!(gets.len(), 2);
        assert!(gets[0].contains("syncToken=expired-token"));
        assert!(gets[1].contains("timeMin=") && !gets[1].contains("syncToken"));
        assert_eq!(cursor_json(&b)["sync_token"], "CPDAlvWDx70CEPDAlvWDx70CGAU=");
        // Unchanged events after a resync aren't new signal again.
        assert!(b.items.iter().all(|i| !i.bump));
    }

    #[test]
    fn new_day_does_a_full_window_sync() {
        let (_, first) = first_sync();
        let mut cur = cursor_json(&first);
        cur["full_on"] = json!("2026-09-27");
        let http = http();
        http.on("GET", "/calendars/primary/events?", 200, fixture!("full_page2.json"));
        let b = poll(&http, json!({}), Some(cur.to_string()));
        let gets: Vec<String> = http.requests().into_iter().filter(|r| r.0 == "GET").map(|r| r.1).collect();
        assert_eq!(gets.len(), 1);
        assert!(gets[0].contains("timeMin="));
        assert_eq!(cursor_json(&b)["full_on"], "2026-09-28");
    }

    #[test]
    fn access_token_is_cached_and_refreshed_when_expired() {
        let http = http();
        http.on("GET", "/calendars/primary/events?", 200, fixture!("full_page2.json"));
        let src = source(json!({ "secret_prefix": "t" }), None);
        let secret = secrets;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: now() };
        let cache = TokenCache::new();
        poll_with(&ctx, &cache).unwrap();
        poll_with(&ctx, &cache).unwrap();
        let token_calls = |h: &FakeHttp| h.requests().iter().filter(|r| r.1.contains("oauth2.googleapis.com/token")).count();
        assert_eq!(token_calls(&http), 1);
        let body = http.requests()[0].2.clone().unwrap();
        assert!(body.contains("grant_type=refresh_token") && body.contains("refresh_token=1%2F%2F0gRefreshTokenExample"));
        // Expire it: the next call refreshes.
        let key = oauth::cache_key("t", "1//0gRefreshTokenExample");
        cache.put(&key, "stale", std::time::Instant::now());
        poll_with(&ctx, &cache).unwrap();
        assert_eq!(token_calls(&http), 2);
    }

    #[test]
    fn revoked_refresh_token_says_how_to_fix_it() {
        let http = FakeHttp::default();
        http.on("POST", "oauth2.googleapis.com/token", 400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#);
        let src = source(json!({}), None);
        let secret = secrets;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: now() };
        let e = poll_with(&ctx, &TokenCache::new()).unwrap_err().to_string();
        assert!(e.contains("gcal.auth_start"), "{e}");
        assert!(!e.contains("GOCSPX") && !e.contains("1//0g"), "no secrets in errors");
    }

    #[test]
    fn missing_refresh_token_points_at_consent() {
        let http = FakeHttp::default();
        let src = source(json!({}), None);
        let secret = |_: &str| None;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: now() };
        let e = poll_with(&ctx, &TokenCache::new()).unwrap_err().to_string();
        assert!(e.contains("gcal.refresh_token") && e.contains("gcal.auth_start"), "{e}");
    }

    #[test]
    fn batch_applies_with_blocks_and_cancellation() {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = crate::settings::SourceConfig {
            kind: "gcal".into(),
            tag: "work".into(),
            name: "Work calendar".into(),
            enabled: true,
            poll_secs: 300,
            config: json!({ "category_by_color": { "3": "self" } }),
        };
        crate::sync::register_sources(&conn, &[cfg]).unwrap();
        let src = crate::sync::load_source(&conn, 1).unwrap();
        let (_, first) = first_sync();
        assert_eq!(crate::sync::apply(&conn, &src, &first).unwrap(), 6);
        let blocks: i64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(blocks, 4, "all-day and declined stay off the strip");
        conn.execute("UPDATE items SET seen = 1", []).unwrap();

        let http = http();
        http.on("GET", "syncToken=", 200, fixture!("incremental.json"));
        let src = crate::sync::load_source(&conn, 1).unwrap();
        let next = poll(&http, src.config.clone(), src.cursor.clone());
        // moved invite + new invite + cancelled lunch
        assert_eq!(crate::sync::apply(&conn, &src, &next).unwrap(), 3);
        let (status, blocks): (String, i64) = conn
            .query_row(
                "SELECT status, (SELECT count(*) FROM events e WHERE e.item_id = i.id) FROM items i WHERE external_id = '2b9vlunch0928'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((status.as_str(), blocks), ("done", 0));
    }
}

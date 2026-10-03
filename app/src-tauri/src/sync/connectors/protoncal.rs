//! Proton Calendar "Limited view" share links (personal): busy blocks only.
//!
//! Each link serves an ICS feed. We expand it into busy intervals for a short
//! window around today and hand back `Batch { busy }` and nothing else. The
//! parser only looks at timing properties (DTSTART, DTEND, DURATION, RRULE,
//! RDATE, EXDATE, RECURRENCE-ID, TRANSP, STATUS). SUMMARY, DESCRIPTION,
//! LOCATION and attendees are never read. The UID is read only to pair a moved
//! instance with its series, and only as a hash that lives for one parse.
//!
//! The links themselves grant access to the calendar (Proton puts the
//! decryption key in the URL), so they never appear in logs or error messages.

use crate::settings::{Settings, SourceConfig};
use crate::sync::{Batch, Busy, Connector, Ctx};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub struct ProtonCalendar;

impl Connector for ProtonCalendar {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        let cfg = &ctx.source.config;
        let urls: Vec<String> = cfg
            .get("urls")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        if urls.is_empty() {
            anyhow::bail!("no calendar links: add Proton \"Limited view\" links to settings.life_calendars");
        }
        let days = |k: &str, d: i64| cfg.get(k).and_then(Value::as_i64).unwrap_or(d).clamp(0, 400);
        // Recurrences are expanded by the `rrule` crate, which works in chrono's
        // types: the window and the local zone go in as chrono values, and the
        // busy times come back out as instants through the clock.
        let today = {
            let d = ctx.clock.today();
            NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32).expect("valid date")
        };
        let zone: chrono_tz::Tz = ctx.clock.tz().iana_name().and_then(|n| n.parse().ok()).unwrap_or(chrono_tz::UTC);
        let window = Window {
            start: today - Duration::days(days("past_days", 1)),
            end: today + Duration::days(days("future_days", 14) + 1),
            skip_all_day: cfg.get("all_day").and_then(Value::as_str) == Some("skip"),
        };

        let mut all = Vec::new();
        for (i, url) in urls.iter().enumerate() {
            // One failed link fails the whole poll: busy blocks are replaced
            // wholesale, so a partial result would silently drop that calendar.
            let n = i + 1;
            let url = url.trim().replacen("webcal://", "https://", 1);
            let resp = ctx
                .http
                .get(&url, &[("accept", "text/calendar")])
                .map_err(|e| anyhow::anyhow!("calendar {n}: request failed ({})", redact(&e.to_string(), &url)))?;
            if !(200..300).contains(&resp.status) {
                anyhow::bail!("calendar {n}: HTTP {}", resp.status);
            }
            if !resp.body.contains("BEGIN:VCALENDAR") {
                anyhow::bail!("calendar {n}: response is not an ICS calendar (is the link still active?)");
            }
            all.extend(busy_from_ics(&resp.body, &window, &zone).map_err(|e| anyhow::anyhow!("calendar {n}: {e}"))?);
        }
        let here = |t: NaiveDateTime| ctx.clock.from_local(t.format("%Y-%m-%dT%H:%M:%S").to_string().parse().expect("a wall-clock time"));
        let busy = merge(all).into_iter().map(|(s, e)| Busy { start: here(s), end: here(e) }).collect();
        Ok(Batch { busy: Some(busy), ..Default::default() })
    }
}

/// Fill in each protoncal source's links from `settings.life_calendars`, and
/// add a source for them when none is configured.
///
/// Auto-adding is deliberate: putting a link in `life_calendars` already says
/// "show this as Life", the source is read-only and can only ever produce
/// untitled busy blocks. Requiring a second, matching entry in `sources`
/// would just be a way to get it wrong. The addition is logged (without links).
pub fn resolve_config(settings: &mut Settings) {
    let links: Vec<Value> = settings.life_calendars.iter().filter(|u| !u.trim().is_empty()).map(|u| json!(u)).collect();
    if !links.is_empty() && !settings.sources.iter().any(|s| s.kind == "protoncal") {
        eprintln!("sync: added personal source 'Life' for {} life_calendars link(s)", links.len());
        settings.sources.push(SourceConfig {
            kind: "protoncal".into(),
            tag: "personal".into(),
            name: "Life".into(),
            enabled: true,
            poll_secs: 900,
            config: json!({}),
        });
    }
    for s in settings.sources.iter_mut().filter(|s| s.kind == "protoncal") {
        if s.tag != "personal" {
            eprintln!("sync: source '{}' is a Proton calendar; treating it as personal", s.name);
            s.tag = "personal".into();
        }
        if !s.config.is_object() {
            s.config = json!({});
        }
        if s.config.get("urls").is_none() {
            s.config["urls"] = Value::Array(links.clone());
        }
    }
}

/// Strip anything link-shaped from a transport error before it's stored.
fn redact(err: &str, url: &str) -> String {
    let mut out = err.replace(url, "<calendar link>");
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    out = out.replace(base, "<calendar link>");
    let path = base.split_once("://").map_or(base, |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
    let secrets = path.split('/').chain(query.split('&')).chain(query.split('&').filter_map(|kv| kv.split_once('=').map(|(_, v)| v)));
    for s in secrets.filter(|s| s.len() >= 8) {
        out = out.replace(s, "…");
    }
    out
}

// ---------------------------------------------------------------- ICS

/// Local dates [start, end) to keep busy time for.
pub struct Window {
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// Drop all-day events (config `all_day: "skip"`). Default: they count as busy.
    pub skip_all_day: bool,
}

/// A DTSTART/DTEND/EXDATE/… value.
#[derive(Debug, Clone, Copy)]
enum When {
    Date(NaiveDate),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, chrono_tz::Tz),
    /// No zone: wall time wherever the user is.
    Floating(NaiveDateTime),
}

impl When {
    /// Date values and floating times are expanded as if they were UTC, which
    /// keeps wall-clock arithmetic exact (no DST); they're read back as naive.
    fn is_floating(&self) -> bool {
        matches!(self, When::Date(_) | When::Floating(_))
    }

    fn naive(self) -> NaiveDateTime {
        match self {
            When::Date(d) => d.and_time(NaiveTime::MIN),
            When::Utc(n) | When::Zoned(n, _) | When::Floating(n) => n,
        }
    }

    fn to_rr(self) -> Option<chrono::DateTime<Tz>> {
        match self {
            When::Date(d) => Some(Tz::UTC.from_utc_datetime(&d.and_time(NaiveTime::MIN))),
            When::Floating(n) | When::Utc(n) => Some(Tz::UTC.from_utc_datetime(&n)),
            When::Zoned(n, tz) => zoned(Tz::Tz(tz), n),
        }
    }
}

/// Resolve a wall time in `tz`. In the autumn overlap take the first; in the
/// spring gap move forward an hour, as calendar apps do.
fn zoned<T: TimeZone>(tz: T, n: NaiveDateTime) -> Option<chrono::DateTime<T>> {
    tz.from_local_datetime(&n).earliest().or_else(|| tz.from_local_datetime(&(n + Duration::hours(1))).earliest())
}

#[derive(Default)]
struct Event {
    start: Option<When>,
    end: Option<When>,
    duration: Option<Duration>,
    rrules: Vec<String>,
    rdates: Vec<When>,
    exdates: Vec<When>,
    recurrence_id: Option<When>,
    uid_hash: u64,
    transparent: bool,
    cancelled: bool,
}

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// RFC 5545 §3.1: a line starting with a space or tab continues the previous one.
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match (line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')), out.last_mut()) {
            (Some(cont), Some(prev)) => prev.push_str(cont),
            _ => out.push(line.to_string()),
        }
    }
    out
}

fn parse_prop(line: &str) -> Option<Prop> {
    // The value starts at the first ':' outside a quoted parameter value.
    let mut quoted = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ':' && !quoted
    })?.0;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value: value.trim().to_string() })
}

/// IANA names, including the `/mozilla.org/…/Europe/Berlin` style prefixes some
/// clients write. Windows zone names aren't mapped: those times become floating.
fn tz_for(tzid: &str) -> Option<chrono_tz::Tz> {
    let tzid = tzid.trim();
    if let Ok(tz) = tzid.parse() {
        return Some(tz);
    }
    let parts: Vec<&str> = tzid.split('/').filter(|s| !s.is_empty()).collect();
    (1..parts.len()).find_map(|i| parts[i..].join("/").parse().ok())
}

fn parse_dt(v: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()
}

fn parse_date(v: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v, "%Y%m%d").ok()
}

/// One value (no commas) with the property's VALUE/TZID parameters.
fn parse_when(v: &str, value_type: Option<&str>, tzid: Option<&str>) -> Option<When> {
    // RDATE;VALUE=PERIOD: keep the start; the event's own duration applies.
    let v = v.split('/').next()?.trim();
    if value_type == Some("DATE") || v.len() == 8 {
        return parse_date(v).map(When::Date);
    }
    if let Some(utc) = v.strip_suffix('Z') {
        return parse_dt(utc).map(When::Utc);
    }
    let n = parse_dt(v)?;
    Some(match tzid.and_then(tz_for) {
        Some(tz) => When::Zoned(n, tz),
        None => When::Floating(n),
    })
}

fn prop_when(p: &Prop) -> Option<When> {
    parse_when(&p.value, p.param("VALUE"), p.param("TZID"))
}

fn prop_whens(p: &Prop) -> Vec<When> {
    p.value.split(',').filter_map(|v| parse_when(v, p.param("VALUE"), p.param("TZID"))).collect()
}

/// `P1W`, `P2D`, `PT1H30M`, `P1DT2H`… (negative durations are ignored).
fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.strip_prefix('+').unwrap_or(v).strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (Duration::zero(), String::new(), false);
    for c in v.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (c, in_time) {
                    ('W', false) => Duration::weeks(n),
                    ('D', false) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(total)
}

/// FNV-1a: a stable, in-memory key for pairing overrides with their series.
fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn parse_events(ics: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut cur: Option<Event> = None;
    // Depth of sub-components (VALARM…) inside the current VEVENT; their
    // properties are skipped.
    let mut nested = 0usize;
    for line in unfold(ics) {
        let Some(p) = parse_prop(&line) else { continue };
        match (p.name.as_str(), p.value.to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") if cur.is_none() => {
                cur = Some(Event::default());
                nested = 0;
                continue;
            }
            ("END", "VEVENT") if nested == 0 => {
                events.extend(cur.take());
                continue;
            }
            ("BEGIN", _) if cur.is_some() => {
                nested += 1;
                continue;
            }
            ("END", _) if cur.is_some() => {
                nested = nested.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let (Some(ev), 0) = (cur.as_mut(), nested) else { continue };
        // Only timing properties are read. Everything else (SUMMARY,
        // DESCRIPTION, LOCATION, ATTENDEE, ORGANIZER…) falls through untouched.
        match p.name.as_str() {
            "DTSTART" => ev.start = prop_when(&p),
            "DTEND" => ev.end = prop_when(&p),
            "DURATION" => ev.duration = parse_duration(&p.value),
            "RRULE" => ev.rrules.push(p.value.clone()),
            "RDATE" => ev.rdates.extend(prop_whens(&p)),
            "EXDATE" => ev.exdates.extend(prop_whens(&p)),
            "RECURRENCE-ID" => ev.recurrence_id = prop_when(&p),
            "UID" => ev.uid_hash = hash(&p.value),
            "TRANSP" => ev.transparent = p.value.eq_ignore_ascii_case("TRANSPARENT"),
            "STATUS" => ev.cancelled = p.value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    events
}

/// A comparable instant: floating values compare as naive wall time.
fn key(dt: &chrono::DateTime<Tz>) -> NaiveDateTime {
    dt.naive_utc()
}

/// Busy intervals (local naive, in `target`) for every opaque, non-cancelled
/// event overlapping the window. Not merged.
pub fn busy_from_ics<T: TimeZone>(ics: &str, window: &Window, target: &T) -> anyhow::Result<Vec<(NaiveDateTime, NaiveDateTime)>> {
    let events = parse_events(ics);
    if events.is_empty() && !ics.contains("BEGIN:VCALENDAR") {
        anyhow::bail!("not an ICS calendar");
    }
    let win_start = window.start.and_time(NaiveTime::MIN);
    let win_end = window.end.and_time(NaiveTime::MIN);

    // Instances moved or cancelled individually, by series.
    let mut overridden: HashMap<u64, HashSet<NaiveDateTime>> = HashMap::new();
    for ev in &events {
        if let Some(rid) = ev.recurrence_id.and_then(When::to_rr) {
            overridden.entry(ev.uid_hash).or_default().insert(key(&rid));
        }
    }

    let mut out = Vec::new();
    for ev in &events {
        if ev.transparent || ev.cancelled {
            continue;
        }
        let Some(start) = ev.start else { continue };
        let all_day = matches!(start, When::Date(_));
        if all_day && window.skip_all_day {
            continue;
        }
        let floating = start.is_floating();
        let Some(dtstart) = start.to_rr() else { continue };
        let length = match (ev.end, ev.duration) {
            // Floating on either side: compare wall times.
            (Some(end), _) if floating || end.is_floating() => end.naive() - start.naive(),
            (Some(end), _) => match end.to_rr() {
                Some(end) => end.signed_duration_since(dtstart),
                None => continue,
            },
            (None, Some(d)) => d,
            (None, None) if all_day => Duration::days(1),
            (None, None) => Duration::zero(),
        };
        if length <= Duration::zero() {
            continue;
        }

        let mut starts = vec![dtstart];
        if ev.recurrence_id.is_none() && (!ev.rrules.is_empty() || !ev.rdates.is_empty()) {
            // Generous bounds in the event's own frame; the exact cut happens
            // after conversion to local time below.
            let lo = Tz::UTC.from_utc_datetime(&(win_start - length - Duration::days(2)));
            let hi = Tz::UTC.from_utc_datetime(&(win_end + Duration::days(2)));
            for rule in &ev.rrules {
                starts.extend(expand(rule, dtstart, &start, lo, hi));
            }
            starts.extend(ev.rdates.iter().filter_map(|w| w.to_rr()));
            let ex: HashSet<NaiveDateTime> = ev.exdates.iter().filter_map(|w| w.to_rr()).map(|d| key(&d)).collect();
            let ex_days: HashSet<NaiveDate> =
                ev.exdates.iter().filter_map(|w| if let When::Date(d) = w { Some(*d) } else { None }).collect();
            let moved = overridden.get(&ev.uid_hash);
            starts.retain(|s| {
                !ex.contains(&key(s))
                    && (all_day || !ex_days.contains(&s.naive_local().date()))
                    && !moved.is_some_and(|m| m.contains(&key(s)))
            });
            starts.sort();
            starts.dedup();
        }

        for s in starts {
            let e = s + length;
            let (ls, le) = if floating {
                (s.naive_utc(), e.naive_utc())
            } else {
                (s.with_timezone(target).naive_local(), e.with_timezone(target).naive_local())
            };
            if le > win_start && ls < win_end {
                out.push((ls, le));
            }
        }
    }
    Ok(out)
}

/// Occurrences of one RRULE between `lo` and `hi`. A rule we can't parse or
/// validate contributes nothing beyond DTSTART (which the caller keeps).
fn expand(rule: &str, dtstart: chrono::DateTime<Tz>, start: &When, lo: chrono::DateTime<Tz>, hi: chrono::DateTime<Tz>) -> Vec<chrono::DateTime<Tz>> {
    // UNTIL is resolved here rather than by the crate, which would read a
    // floating UNTIL in this machine's zone.
    let mut until = None;
    let parts: Vec<&str> = rule
        .split(';')
        .filter(|kv| match kv.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("UNTIL") => {
                until = until_utc(v, start);
                false
            }
            _ => true,
        })
        .collect();
    let Ok(mut rr) = parts.join(";").parse::<RRule<Unvalidated>>() else { return vec![] };
    if let Some(u) = until {
        rr = rr.until(u);
    }
    let Ok(rr) = rr.validate(dtstart) else { return vec![] };
    let lo = if lo < dtstart { dtstart } else { lo.with_timezone(&dtstart.timezone()) };
    RRuleSet::new(dtstart).rrule(rr).after(lo).before(hi.with_timezone(&dtstart.timezone())).all(2000).dates
}

/// UNTIL as UTC (what the crate requires), read in the frame of DTSTART.
fn until_utc(v: &str, start: &When) -> Option<chrono::DateTime<Tz>> {
    let u = parse_when(v, None, None)?;
    let naive_utc = match (u, start) {
        (When::Utc(n), _) => n,
        // Floating / all-day series are expanded as UTC wall time.
        (When::Date(d), When::Date(_)) => d.and_time(NaiveTime::MIN),
        (When::Floating(n), When::Date(_) | When::Floating(_) | When::Utc(_)) => n,
        (When::Date(d), When::Floating(_) | When::Utc(_)) => d.and_hms_opt(23, 59, 59)?,
        (When::Date(d), When::Zoned(_, tz)) => zoned(*tz, d.and_hms_opt(23, 59, 59)?)?.naive_utc(),
        (When::Floating(n), When::Zoned(_, tz)) => zoned(*tz, n)?.naive_utc(),
        (When::Zoned(n, tz), _) => zoned(tz, n)?.naive_utc(),
    };
    Some(Tz::UTC.from_utc_datetime(&naive_utc))
}

/// Sort and merge overlapping or touching intervals.
pub fn merge(mut v: Vec<(NaiveDateTime, NaiveDateTime)>) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    v.sort();
    let mut out: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::with_capacity(v.len());
    for (s, e) in v {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;
    use crate::sync::Source;

    const ZURICH: chrono_tz::Tz = chrono_tz::Europe::Zurich;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
    }

    fn window(start: &str, days: i64) -> Window {
        let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").unwrap();
        Window { start, end: start + Duration::days(days), skip_all_day: false }
    }

    fn busy(ics: &str, w: &Window) -> Vec<(String, String)> {
        merge(busy_from_ics(ics, w, &ZURICH).unwrap())
            .into_iter()
            .map(|(s, e)| (s.format("%m-%d %H:%M").to_string(), e.format("%m-%d %H:%M").to_string()))
            .collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    const TIMEZONES: &str = include_str!("../../../tests/fixtures/protoncal/timezones.ics");
    const RECURRING: &str = include_str!("../../../tests/fixtures/protoncal/recurring.ics");
    const MIXED: &str = include_str!("../../../tests/fixtures/protoncal/mixed.ics");

    #[test]
    fn converts_zones_including_dst_boundary() {
        // Daily 09:00–10:00 New York from Oct 30 to Nov 2 2026. Zurich leaves
        // DST on Oct 25, New York on Nov 1, so the local time shifts by an hour.
        let w = window("2026-10-29", 6);
        let got = busy(TIMEZONES, &w);
        assert_eq!(
            got,
            pairs(&[
                ("10-29 12:00", "10-29 12:30"), // UTC event
                ("10-30 14:00", "10-30 15:00"),
                ("10-31 14:00", "10-31 15:00"),
                ("11-01 15:00", "11-01 16:00"),
                ("11-02 15:00", "11-02 16:00"),
                ("11-03 08:00", "11-03 09:00"), // floating: wall time as written
                ("11-03 18:00", "11-03 19:00"), // mozilla-style TZID prefix, Berlin
            ])
        );
    }

    #[test]
    fn recurrence_with_exdate_rdate_and_moved_instance() {
        let w = window("2026-09-27", 16);
        let got = busy(RECURRING, &w);
        assert_eq!(
            got,
            pairs(&[
                // Weekly Mon 07:30–08:15 Zurich, Sep 28 excluded, Oct 5 moved to 10:00,
                // Oct 12 cancelled, plus an extra RDATE on Oct 8.
                ("10-05 10:00", "10-05 10:45"),
                ("10-08 07:30", "10-08 08:15"),
                // Every other day with COUNT=3 starting Sep 20: all before the window.
            ])
        );
    }

    #[test]
    fn all_day_transparent_cancelled_and_merge() {
        let w = window("2026-09-27", 16);
        let got = busy(MIXED, &w);
        assert_eq!(
            got,
            pairs(&[
                // Two overlapping + one touching block merge into one.
                ("09-28 17:00", "09-28 19:30"),
                // Two-day all-day event: local midnight to midnight.
                ("10-02 00:00", "10-04 00:00"),
            ])
        );
        let skip = Window { skip_all_day: true, ..window("2026-09-27", 16) };
        assert_eq!(busy(MIXED, &skip).len(), 1);
    }

    #[test]
    fn nested_alarm_and_folded_lines_are_handled() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Europe/Zurich:20260928T120000\r\nDTEND;TZID=Europe/Zu\r\n rich:20260928T130000\r\nBEGIN:VALARM\r\nDTSTART:20000101T000000Z\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(busy(ics, &window("2026-09-28", 1)), pairs(&[("09-28 12:00", "09-28 13:00")]));
    }

    #[test]
    fn merge_sorts_and_joins() {
        let m = merge(vec![
            (dt("2026-09-28T13:00"), dt("2026-09-28T14:00")),
            (dt("2026-09-28T09:00"), dt("2026-09-28T10:00")),
            (dt("2026-09-28T09:30"), dt("2026-09-28T09:45")),
            (dt("2026-09-28T10:00"), dt("2026-09-28T11:00")),
        ]);
        assert_eq!(m, vec![(dt("2026-09-28T09:00"), dt("2026-09-28T11:00")), (dt("2026-09-28T13:00"), dt("2026-09-28T14:00"))]);
    }

    fn source(config: Value) -> Source {
        Source { id: 1, kind: "protoncal".into(), tag: "personal".into(), name: "Life".into(), config, cursor: None }
    }

    const LINK: &str = "https://calendar.proton.me/api/calendar/v1/url/AbCdEfGhIjKlMnOp/calendar.ics?CacheKey=q1w2e3r4t5y6&PassphraseKey=Zx9Yw8Vu7Ts6";

    #[test]
    fn poll_returns_only_untitled_busy_blocks() {
        let http = FakeHttp::default();
        http.on("GET", "/url/AbCdEfGhIjKlMnOp/", 200, MIXED);
        http.on("GET", "/url/SecondCalendar00/", 200, TIMEZONES);
        let src = source(json!({ "urls": [LINK, "webcal://calendar.proton.me/api/calendar/v1/url/SecondCalendar00/calendar.ics?x=1"] }));
        let secret = |_: &str| None;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: crate::time::t("2026-09-28T10:52:00"), clock: crate::sync::test_clock("2026-09-28T10:52:00Z") };
        let batch = ProtonCalendar.poll(&ctx).unwrap();
        assert!(batch.items.is_empty());
        assert!(batch.cursor.is_none());
        let busy = batch.busy.as_ref().unwrap();
        assert!(!busy.is_empty());
        // Titles, descriptions, locations, attendees and UIDs from the fixtures
        // appear nowhere in what we return.
        let dump = format!("{batch:?}");
        for secret_text in [
            "Physio", "Pickup", "Dentist", "Birthday", "Holiday", "therapist", "Hauptstrasse", "friend@example.com", "Sam",
            "uid-", "@proton.me", "SUMMARY", "DESCRIPTION", "LOCATION",
        ] {
            assert!(!dump.contains(secret_text), "{secret_text} leaked into {dump}");
        }
        for fixture in [MIXED, TIMEZONES] {
            assert!(fixture.contains("SUMMARY:"), "fixtures must carry titles for this test to mean anything");
        }
        // webcal:// is fetched over https.
        assert!(http.requests().iter().all(|(_, u, _)| u.starts_with("https://")));
    }

    #[test]
    fn apply_stores_untitled_life_blocks() {
        let conn = crate::db::open_in_memory().unwrap();
        let mut settings = Settings { life_calendars: vec![LINK.into()], ..Default::default() };
        resolve_config(&mut settings);
        crate::sync::register_sources(&conn, &settings.sources).unwrap();
        let src = crate::sync::load_source(&conn, 1).unwrap();
        assert_eq!((src.kind.as_str(), src.tag.as_str()), ("protoncal", "personal"));
        let http = FakeHttp::default();
        http.on("GET", "calendar.proton.me", 200, MIXED);
        let secret = |_: &str| None;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: crate::time::t("2026-09-28T10:52:00"), clock: crate::sync::test_clock("2026-09-28T10:52:00Z") };
        let batch = ProtonCalendar.poll(&ctx).unwrap();
        crate::sync::apply(&conn, &src, &batch, crate::time::t("2026-09-28T10:52:00")).unwrap();
        let (n, titled, items): (i64, i64, i64) = conn
            .query_row(
                "SELECT count(*), count(title), (SELECT count(*) FROM items) FROM events WHERE is_life_busy = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((n, titled, items), (2, 0, 0));
    }

    #[test]
    fn errors_never_contain_the_link() {
        let http = FakeHttp::default();
        http.on("GET", "calendar.proton.me", 404, "<html>Not found: /api/calendar/v1/url/AbCdEfGhIjKlMnOp</html>");
        let src = source(json!({ "urls": [LINK] }));
        let secret = |_: &str| None;
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: crate::time::t("2026-09-28T10:52:00"), clock: crate::sync::test_clock("2026-09-28T10:52:00Z") };
        let err = ProtonCalendar.poll(&ctx).unwrap_err().to_string();
        assert_eq!(err, "calendar 1: HTTP 404");

        // No route: FakeHttp's error message includes the URL; it must be redacted.
        let http = FakeHttp::default();
        let ctx = Ctx { source: &src, http: &http, secret: &secret, now: crate::time::t("2026-09-28T10:52:00"), clock: crate::sync::test_clock("2026-09-28T10:52:00Z") };
        let err = ProtonCalendar.poll(&ctx).unwrap_err().to_string();
        for part in ["AbCdEfGhIjKlMnOp", "q1w2e3r4t5y6", "Zx9Yw8Vu7Ts6"] {
            assert!(!err.contains(part), "{err}");
        }
    }

    #[test]
    fn resolve_adds_source_and_fills_links() {
        let mut s = Settings { life_calendars: vec![LINK.into()], ..Default::default() };
        resolve_config(&mut s);
        assert_eq!(s.sources.len(), 1);
        assert_eq!(s.sources[0].config["urls"], json!([LINK]));
        // An explicit source is kept (and forced personal); its own urls win.
        let mut s = Settings {
            life_calendars: vec![LINK.into()],
            sources: vec![SourceConfig {
                kind: "protoncal".into(),
                tag: "work".into(),
                name: "Family".into(),
                enabled: true,
                poll_secs: 600,
                config: json!({ "urls": ["https://example.test/a.ics"] }),
            }],
            ..Default::default()
        };
        resolve_config(&mut s);
        assert_eq!(s.sources.len(), 1);
        assert_eq!(s.sources[0].tag, "personal");
        assert_eq!(s.sources[0].config["urls"], json!(["https://example.test/a.ics"]));
        // No links, no source.
        let mut s = Settings::default();
        resolve_config(&mut s);
        assert!(s.sources.is_empty());
    }
}

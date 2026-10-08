//! Read models and actions for each view. Each module exposes
//! `call(app, name, args)` and a `seed(ctx)` for its sample data.

pub mod categories;
pub mod daystrip;
pub mod incoming;
pub mod notes;
pub mod people;
pub mod priorities;
pub mod reflect;
pub mod respond;
pub mod search;
pub mod split;
pub mod todos;
pub mod topics;

use crate::clock::Clock;
use crate::error::{msg, Result};
use crate::time::{Date, Due, Ts};
use jiff::civil::DateTime;
use serde::Serialize;
use serde_json::Value;

pub fn arg_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| msg(format!("missing string arg '{key}'")))
}

pub fn arg_i64(args: &Value, key: &str) -> Result<i64> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| msg(format!("missing int arg '{key}'")))
}

pub fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

/// A stored instant (`crate::time`).
pub fn parse_ts(s: &str) -> Option<Ts> {
    Ts::parse(s)
}

pub fn parse_date(s: &str) -> Result<Date> {
    s.parse::<Date>().map_err(|_| msg(format!("bad date '{s}'")))
}

/// Minutes since midnight of a wall-clock time.
pub fn minute_of_day(t: DateTime) -> i64 {
    t.hour() as i64 * 60 + t.minute() as i64
}

/// Human label for a wall-clock time relative to `now` (both from
/// `Clock::local`): "Today 16:45", "Thu 14:00", "Sep 25", "Wed" (date only).
pub fn when_label(t: DateTime, now: DateTime, with_time: bool) -> String {
    let days = (t.date() - now.date()).get_days();
    let time = t.strftime("%H:%M");
    match days {
        0 if with_time => format!("Today {time}"),
        0 => "Today".into(),
        1..=6 if with_time => format!("{} {time}", t.strftime("%a")),
        1..=6 => t.strftime("%a").to_string(),
        _ => t.strftime("%b %-d").to_string(),
    }
}

/// An instant's label here, relative to now.
pub fn ts_label(t: Ts, clock: &Clock, with_time: bool) -> String {
    when_label(clock.local(t), clock.local_now(), with_time)
}

/// "today", "Wed", "Oct 9", plus the time when the due date has one ("today 15:00").
pub fn due_label(due: Due, clock: &Clock) -> String {
    let now = clock.local_now();
    match due {
        Due::Day(d) => when_label(d.at(12, 0, 0, 0), now, false).replace("Today", "today"),
        Due::At(t) => {
            let t = clock.local(t);
            format!(
                "{} {}",
                when_label(t, now, false).replace("Today", "today"),
                t.strftime("%H:%M")
            )
        }
    }
}

/// "45m", "1h30m".
pub fn dur_label(mins: i64) -> String {
    match (mins / 60, mins % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Pill {
    pub label: String,
    /// red | yellow | green: how much this reason adds.
    pub weight: String,
}

pub use crate::domain::topics::TopicRef;

pub fn topic_ref(r: &dyn crate::domain::ports::Reads, id: i64) -> Result<TopicRef> {
    r.topics().get_ref(id)?.ok_or_else(|| msg("no such topic"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_labels_show_time_only_when_there_is_one() {
        let la = jiff::tz::TimeZone::get("America/Los_Angeles").unwrap();
        let clock = Clock::fixed(Ts::parse("2026-09-28T17:52:00Z").unwrap(), la); // 10:52 there
        let day = |s: &str| Due::Day(s.parse().unwrap());
        let at = |s: &str| Due::At(Ts::parse(s).unwrap());
        assert_eq!(due_label(day("2026-09-28"), &clock), "today");
        assert_eq!(due_label(at("2026-09-28T22:00:00Z"), &clock), "today 15:00");
        assert_eq!(due_label(day("2026-09-30"), &clock), "Wed");
        assert_eq!(due_label(at("2026-10-01T00:30:00Z"), &clock), "Wed 17:30");
        assert_eq!(due_label(day("2026-10-09"), &clock), "Oct 9");
        // Moving to Zurich (19:52 there): a calendar day stays that day; an instant moves with the wall clock.
        let zurich = Clock::fixed(
            Ts::parse("2026-09-28T17:52:00Z").unwrap(),
            jiff::tz::TimeZone::get("Europe/Zurich").unwrap(),
        );
        assert_eq!(due_label(day("2026-09-28"), &zurich), "today");
        assert_eq!(due_label(at("2026-09-28T22:00:00Z"), &zurich), "Tue 00:00");
        // A day-only due is due by the end of that day where you are.
        assert_eq!(day("2026-09-28").deadline(&clock).to_db(), "2026-09-29T06:59:59.999Z");
        assert_eq!(day("2026-09-28").deadline(&zurich).to_db(), "2026-09-28T21:59:59.999Z");
    }
}

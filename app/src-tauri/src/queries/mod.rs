//! Read models and actions for each view. Each module exposes
//! `call(app, name, args)` and a `seed(ctx)` for its sample data.

pub mod daystrip;
pub mod incoming;
pub mod notes;
pub mod priorities;
pub mod reflect;
pub mod search;
pub mod split;
pub mod topics;

use crate::error::{msg, Result};
use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;
use serde_json::Value;

pub fn arg_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| msg(format!("missing string arg '{key}'")))
}

pub fn arg_i64(args: &Value, key: &str) -> Result<i64> {
    args.get(key).and_then(Value::as_i64).ok_or_else(|| msg(format!("missing int arg '{key}'")))
}

pub fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

pub fn parse_ts(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()
}

pub fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| msg(format!("bad date '{s}'")))
}

/// Minutes since local midnight.
pub fn minute_of_day(t: NaiveDateTime) -> i64 {
    use chrono::Timelike;
    (t.hour() * 60 + t.minute()) as i64
}

/// Human label relative to `now`: "Today 16:45", "Thu 14:00", "Sep 25", "Wed" (date only).
pub fn when_label(t: NaiveDateTime, now: NaiveDateTime, with_time: bool) -> String {
    let days = (t.date() - now.date()).num_days();
    let time = t.format("%H:%M");
    match days {
        0 if with_time => format!("Today {time}"),
        0 => "Today".into(),
        1..=6 if with_time => format!("{} {time}", t.format("%a")),
        1..=6 => t.format("%a").to_string(),
        _ => t.format("%b %-d").to_string(),
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

#[derive(Debug, Clone, Serialize)]
pub struct TopicRef {
    pub id: i64,
    pub name: String,
    pub category: String,
}

pub fn topic_ref(conn: &rusqlite::Connection, id: i64) -> Result<TopicRef> {
    Ok(conn.query_row("SELECT id, name, category FROM topics WHERE id = ?1", [id], |r| {
        Ok(TopicRef { id: r.get(0)?, name: r.get(1)?, category: r.get(2)? })
    })?)
}

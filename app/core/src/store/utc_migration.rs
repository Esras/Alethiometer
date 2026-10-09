//! Migration 21: every stored time becomes a UTC instant (`crate::time`).
//!
//! Before this, instants were stored as naive local wall-clock strings
//! ("2026-09-28T10:52:00"), except a few columns that took SQLite's
//! `datetime('now')`, which is UTC ("2026-09-28 08:52:00"). Each value is read
//! in the zone it was written in (the system zone for local ones) and stored
//! as `YYYY-MM-DDTHH:MM:SS.sssZ`.
//!
//! - Local times in the repeated hour of a DST fall-back are ambiguous; they
//!   take the first occurrence (and are counted). Times skipped by a spring
//!   forward are read past the gap.
//! - Local times inside JSON the app wrote (agent transcripts and sessions,
//!   items' raw fields, cursors, outbox payloads, Notes lines) are converted
//!   the same way: any string that is exactly a naive date-time. API payloads
//!   kept in raw fields always carry an offset and are left alone.
//! - Google Calendar's cursor keeps what each event looked like at the last
//!   poll; its times are converted and its fingerprints recomputed from the
//!   stored event (`gcal::fingerprint`), so nothing resurfaces as changed.
//! - Message keys that hash a message's time (`split::content_key`) are
//!   recomputed from the converted time, so splits and read marks still match.

use crate::time::Ts;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use rusqlite::{params, Connection};
use serde_json::Value;

/// Columns written in local time.
const LOCAL: &[(&str, &str)] = &[
    ("items", "occurred_at"),
    ("items", "ended_at"),
    ("items", "due_at"),
    ("item_messages", "at"),
    ("events", "start"),
    ("events", "end"),
    ("outbox", "sent_at"),
    ("outbox", "confirmed_at"),
    ("ai_usage", "at"),
    ("ai_suggest_run", "at"),
    ("ai_suggest_run", "last_ok_at"),
    ("held_todos", "created_at"),
    ("todo_estimates", "updated_at"),
    ("embed_meta", "at"),
    ("search_feedback", "at"),
    ("sources", "last_sync_at"),
    ("person_snoozes", "until"),
];

/// Columns written by SQLite's `datetime('now')`: already UTC.
const UTC: &[(&str, &str)] = &[
    ("topics", "archived_at"),
    ("topic_links", "updated_at"),
    ("priorities", "updated_at"),
    ("outbox", "created_at"),
    ("account_profiles", "updated_at"),
];

/// JSON columns the app writes times into.
const JSON: &[(&str, &str)] = &[
    ("items", "raw_json"),
    ("outbox", "payload_json"),
    ("outbox", "receipt_json"),
    ("notes_lines", "target_json"),
    ("agent_rows", "data"),
    ("agent_sessions", "data"),
    ("agent_groups", "data"),
    ("agent_hosts", "turn"),
];

#[derive(Default, Debug)]
pub struct Report {
    pub converted: usize,
    /// Local times in a repeated DST hour, read as the first occurrence.
    pub ambiguous: usize,
    /// Values that weren't a time at all, stored as the Unix epoch.
    pub unreadable: usize,
}

pub fn run(conn: &Connection) -> anyhow::Result<()> {
    let r = run_in(conn, &TimeZone::system())?;
    eprintln!(
        "migrated {} stored times to UTC ({} in a repeated DST hour read as the first; {} unreadable, set to the epoch)",
        r.converted, r.ambiguous, r.unreadable
    );
    Ok(())
}

pub fn run_in(conn: &Connection, tz: &TimeZone) -> anyhow::Result<Report> {
    let mut r = Report::default();
    for (local, cols) in [(true, LOCAL), (false, UTC)] {
        for (table, col) in cols {
            let rows: Vec<(i64, String)> = conn
                .prepare(&format!(
                    "SELECT rowid, \"{col}\" FROM {table} WHERE \"{col}\" IS NOT NULL AND \"{col}\" NOT LIKE '%Z'"
                ))?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (rowid, v) in rows {
                let new = convert(&v, local, tz, &mut r).unwrap_or_else(|| {
                    r.unreadable += 1;
                    Ts::default()
                });
                conn.execute(
                    &format!("UPDATE {table} SET \"{col}\" = ?1 WHERE rowid = ?2"),
                    params![new, rowid],
                )?;
            }
        }
    }
    for (table, col) in JSON {
        let rows: Vec<(i64, String)> = conn
            .prepare(&format!("SELECT rowid, {col} FROM {table} WHERE {col} IS NOT NULL"))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (rowid, text) in rows {
            let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if walk(&mut v, tz, &mut r) {
                conn.execute(
                    &format!("UPDATE {table} SET {col} = ?1 WHERE rowid = ?2"),
                    params![v.to_string(), rowid],
                )?;
            }
        }
    }
    cursors(conn, tz, &mut r)?;
    message_keys(conn)?;
    Ok(r)
}

/// "2026-09-28T10:52:00" or "2026-09-28 10:52:00" (fractions allowed) → UTC.
fn convert(v: &str, local: bool, tz: &TimeZone, r: &mut Report) -> Option<Ts> {
    let dt: DateTime = v.trim().replacen(' ', "T", 1).parse().ok()?;
    let zone = if local { tz.clone() } else { TimeZone::UTC };
    let amb = zone.to_ambiguous_timestamp(dt);
    if matches!(amb.offset(), jiff::tz::AmbiguousOffset::Fold { .. }) {
        r.ambiguous += 1;
    }
    let ts = Ts::new(amb.compatible().ok()?);
    r.converted += 1;
    Some(ts)
}

fn naive(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 19
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && s.chars()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit())
}

/// Convert every naive local date-time string in `v`. Returns whether anything changed.
fn walk(v: &mut Value, tz: &TimeZone, r: &mut Report) -> bool {
    match v {
        Value::String(s) if naive(s) => match convert(s, true, tz, r) {
            Some(t) => {
                *s = t.to_db();
                true
            }
            None => false,
        },
        Value::Array(a) => a.iter_mut().fold(false, |ch, x| walk(x, tz, r) | ch),
        Value::Object(o) => o.values_mut().fold(false, |ch, x| walk(x, tz, r) | ch),
        _ => false,
    }
}

/// Cursors: naive times anywhere in a JSON cursor, and Google Calendar's
/// pipe-separated `seen` entries ("end|hash|n|start|guests|change").
fn cursors(conn: &Connection, tz: &TimeZone, r: &mut Report) -> anyhow::Result<()> {
    let rows: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, kind, cursor FROM sources WHERE cursor IS NOT NULL")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, kind, text) in rows {
        let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let mut changed = walk(&mut v, tz, r);
        if kind == "gcal" {
            if let Some(seen) = v.get_mut("seen").and_then(Value::as_object_mut) {
                for (event, entry) in seen.iter_mut() {
                    let Some(s) = entry.as_str() else { continue };
                    let mut parts: Vec<String> = s.split('|').map(str::to_owned).collect();
                    let mut time = |p: &mut String| {
                        if naive(p) {
                            if let Some(t) = convert(p, true, tz, r) {
                                *p = t.to_db();
                            }
                        }
                    };
                    if let Some(end) = parts.get_mut(0) {
                        time(end);
                    }
                    if let Some(start) = parts.get_mut(3) {
                        time(start);
                    }
                    if let Some(change) = parts.get_mut(5) {
                        if let Some(from) = change.strip_prefix("moved@") {
                            let mut from = from.to_owned();
                            time(&mut from);
                            *change = format!("moved@{from}");
                        }
                    }
                    // The fingerprint is now over the API's own start and end: recompute it from the stored event.
                    let raw: Option<String> = conn
                        .query_row(
                            "SELECT raw_json FROM items WHERE source_id = ?1 AND external_id = ?2",
                            params![id, event],
                            |row| row.get(0),
                        )
                        .ok();
                    if let (Some(raw), Some(hash)) = (
                        raw.and_then(|t| serde_json::from_str::<Value>(&t).ok()),
                        parts.get_mut(1),
                    ) {
                        *hash = gcal_fingerprint(&raw);
                    }
                    *entry = Value::String(parts.join("|"));
                    changed = true;
                }
            }
        }
        if changed {
            conn.execute(
                "UPDATE sources SET cursor = ?1 WHERE id = ?2",
                params![v.to_string(), id],
            )?;
        }
    }
    Ok(())
}

/// Keys that hash a message's time ("h:…") follow its converted time.
fn message_keys(conn: &Connection) -> anyhow::Result<()> {
    let rows: Vec<(i64, String, String, String)> = conn
        .prepare("SELECT id, at, author, text FROM item_messages WHERE ext_key LIKE 'h:%'")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, at, author, text) in rows {
        conn.execute(
            "UPDATE item_messages SET ext_key = ?1 WHERE id = ?2",
            params![crate::domain::splits::content_key(&at, &author, &text), id],
        )?;
    }
    Ok(())
}

/// The Google Calendar connector's event fingerprint as it was when this migration was written. Frozen here so the
/// migration doesn't change if the connector's fingerprint does.
fn gcal_fingerprint(ev: &Value) -> String {
    let mut emails: Vec<&str> = ev
        .get("attendees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| {
            a.get("self").and_then(Value::as_bool) != Some(true)
                && a.get("resource").and_then(Value::as_bool) != Some(true)
        })
        .filter_map(|a| a.get("email").and_then(Value::as_str))
        .collect();
    emails.sort_unstable();
    let status = ev.get("status").and_then(Value::as_str).unwrap_or("confirmed");
    let (start, end) = (
        ev.get("start").map(Value::to_string).unwrap_or_default(),
        ev.get("end").map(Value::to_string).unwrap_or_default(),
    );
    let key = format!("{start}|{end}|{status}|{}", emails.join(","));
    let fnv = key.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("{:08x}", fnv as u32)
}

#[cfg(test)]
mod tests;

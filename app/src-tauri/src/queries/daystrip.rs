//! Day strip: today's blocks, to scale. Life blocks carry no title (rule 3).

use super::{arg_i64, minute_of_day, opt_i64, parse_ts, when_label};
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::state::App;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
pub struct Block {
    pub id: i64,
    pub start_min: i64,
    pub end_min: i64,
    pub title: Option<String>,
    pub category: String,
    pub tentative: bool,
    pub life: bool,
    /// Travel buffer before a Life block (override or default).
    pub buffer_min: Option<i64>,
    pub topic_ids: Vec<i64>,
    pub item_id: Option<i64>,
    pub attendees: Option<String>,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "get" => get(app, opt_i64(args, "topic_id")),
        "set_buffer" => {
            let id = arg_i64(args, "event_id")?;
            let minutes = arg_i64(args, "minutes")?;
            if !(0..=240).contains(&minutes) {
                return Err(msg("buffer must be 0–240 minutes"));
            }
            app.db().execute("UPDATE events SET buffer_min = ?1 WHERE id = ?2 AND is_life_busy = 1", params![minutes, id])?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(unknown(name)),
    }
}

fn get(app: &App, topic_id: Option<i64>) -> Result<Value> {
    let now = app.clock.now();
    let today = now.date().format("%Y-%m-%d").to_string();
    let conn = app.db();
    let mut stmt = conn.prepare(
        "SELECT e.id, e.start, e.end, e.title, e.category, e.tentative, e.is_life_busy, e.buffer_min, e.item_id, e.attendees,
                (SELECT group_concat(tl.topic_id) FROM topic_links tl
                  WHERE tl.item_id = e.item_id AND tl.state != 'wrong')
           FROM events e
          WHERE substr(e.start, 1, 10) = ?1
          ORDER BY e.start",
    )?;
    let default_buffer = app.settings().life_buffer_min;
    let blocks = stmt
        .query_map([&today], |r| {
            let start: String = r.get(1)?;
            let end: String = r.get(2)?;
            let life: bool = r.get::<_, i64>(6)? == 1;
            let topics: Option<String> = r.get(10)?;
            Ok(Block {
                id: r.get(0)?,
                start_min: parse_ts(&start).map(minute_of_day).unwrap_or(0),
                end_min: parse_ts(&end).map(minute_of_day).unwrap_or(0),
                title: if life { None } else { r.get(3)? },
                category: r.get(4)?,
                tentative: r.get::<_, i64>(5)? == 1,
                life,
                buffer_min: if life { Some(r.get::<_, Option<i64>>(7)?.unwrap_or(default_buffer)) } else { None },
                item_id: r.get(8)?,
                attendees: if life { None } else { r.get(9)? },
                topic_ids: topics
                    .map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // 7b: when the current topic has nothing (left) today, name its next linked meeting.
    let mut note = None;
    if let Some(tid) = topic_id {
        let linked = |b: &&Block| b.topic_ids.contains(&tid);
        let earlier = blocks.iter().any(|b| linked(&b));
        if !blocks.iter().filter(linked).any(|b| b.end_min > minute_of_day(now)) {
            let next: Option<String> = conn
                .query_row(
                    "SELECT e.start FROM events e JOIN topic_links tl ON tl.item_id = e.item_id
                      WHERE tl.topic_id = ?1 AND tl.state != 'wrong' AND e.start > ?2
                      ORDER BY e.start LIMIT 1",
                    params![tid, crate::seed::fmt(now)],
                    |r| r.get(0),
                )
                .ok();
            note = Some(match next.as_deref().and_then(parse_ts) {
                Some(t) => format!("No {}linked meetings today · next {}", if earlier { "more " } else { "" }, when_label(t, now, true)),
                None => format!("No {}linked meetings today", if earlier { "more " } else { "" }),
            });
        }
    }

    Ok(json!({
        "day_start_min": app.settings().day_start_hour * 60,
        "day_end_min": app.settings().day_end_hour * 60,
        "blocks": blocks,
        "note": note,
    }))
}

//! Day strip: today's blocks, to scale. Life blocks carry no title (rule 3).

use super::{arg_i64, minute_of_day, opt_i64, ts_label};
use crate::api::{Ctx, Route};
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::time::Ts;
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

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "get" => Route::Read(|r, ctx, args| get(r, ctx, opt_i64(args, "topic_id"))),
        "set_buffer" => Route::Write(|uow, _, args| {
            let id = arg_i64(args, "event_id")?;
            let minutes = arg_i64(args, "minutes")?;
            set_buffer(uow, id, minutes)
        }),
        _ => return None,
    })
}

fn set_buffer(uow: &dyn UnitOfWork, event_id: i64, minutes: i64) -> Result<Value> {
    if !(0..=240).contains(&minutes) {
        return Err(msg("buffer must be 0–240 minutes"));
    }
    if !uow.events().set_buffer(event_id, minutes)? {
        return Err(msg("only a life block has a buffer"));
    }
    Ok(json!({ "ok": true }))
}

fn get(r: &dyn Reads, ctx: &Ctx, topic_id: Option<i64>) -> Result<Value> {
    let clock = ctx.clock;
    let now = clock.now();
    let (day_start, day_end) = clock.day_range(clock.today());
    // Minutes since midnight here; a block running past midnight ends at the day's end.
    let minute = |t: Ts| if t >= day_end { 24 * 60 } else { minute_of_day(clock.local(t)) };
    let blocks: Vec<Block> = r
        .day_strip()
        .blocks(day_start, day_end)?
        .into_iter()
        .map(|e| Block {
            id: e.id,
            start_min: minute(e.start),
            // A block ending in the repeated hour of a fall-back can end "before" it starts on the wall.
            end_min: minute(e.end).max(minute(e.start)),
            title: if e.life { None } else { e.title },
            category: e.category,
            tentative: e.tentative,
            life: e.life,
            buffer_min: e.life.then(|| e.buffer_min.unwrap_or(ctx.settings.life_buffer_min)),
            item_id: e.item_id,
            attendees: if e.life { None } else { e.attendees },
            topic_ids: e.topic_ids,
        })
        .collect();

    // 7b: when the current topic has nothing (left) today, name its next linked meeting.
    let mut note = None;
    if let Some(tid) = topic_id {
        let linked = |b: &&Block| b.topic_ids.contains(&tid);
        let earlier = blocks.iter().any(|b| linked(&b));
        if !blocks.iter().filter(linked).any(|b| b.end_min > minute(now)) {
            let more = if earlier { "more " } else { "" };
            note = Some(match r.day_strip().next_linked_meeting(tid, now)? {
                Some(t) => format!("No {more}linked meetings today · next {}", ts_label(t, clock, true)),
                None => format!("No {more}linked meetings today"),
            });
        }
    }

    Ok(json!({
        "day_start_min": ctx.settings.day_start_hour * 60,
        "day_end_min": ctx.settings.day_end_hour * 60,
        "blocks": blocks,
        "note": note,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::state::App;
    use rusqlite::params;
    use crate::settings::Settings;
    use jiff::tz::TimeZone;

    fn blocks(app: &App) -> Vec<(String, i64, i64)> {
        let v = crate::api::call(app, "daystrip.get", &json!({})).unwrap();
        v["blocks"].as_array().unwrap().iter().filter(|b| b["title"].as_str().is_some_and(|t| t.starts_with("dst-"))).map(|b| (b["title"].as_str().unwrap().to_owned(), b["start_min"].as_i64().unwrap(), b["end_min"].as_i64().unwrap())).collect()
    }

    #[test]
    fn the_strip_on_a_fall_back_day_and_from_another_zone() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        // Nov 1 2026, 10:00 in Los Angeles: the day runs 07:00Z to 08:00Z the next day (25 hours).
        app.clock = Clock::fixed(Ts::parse("2026-11-01T18:00:00Z").unwrap(), TimeZone::get("America/Los_Angeles").unwrap());
        {
            let conn = app.db();
            conn.execute("INSERT INTO sources (id, kind, tag, name) VALUES (99, 'gcal', 'work', 'cal')", []).unwrap();
            for (title, start, end) in [
                ("dst-before", "2026-11-01T06:30:00Z", "2026-11-01T07:00:00Z"), // Oct 31, 23:30 PDT
                ("dst-fold", "2026-11-01T08:30:00Z", "2026-11-01T09:15:00Z"),   // 01:30 PDT to 01:15 PST
                ("dst-morning", "2026-11-01T17:00:00Z", "2026-11-01T18:00:00Z"), // 09:00 PST
                ("dst-late", "2026-11-02T07:30:00Z", "2026-11-02T08:30:00Z"),   // 23:30 PST into Nov 2
                ("dst-after", "2026-11-02T08:30:00Z", "2026-11-02T09:00:00Z"),  // Nov 2
            ] {
                let (start, end) = (Ts::parse(start).unwrap(), Ts::parse(end).unwrap());
                conn.execute("INSERT INTO events (source_id, start, end, title, category) VALUES (99, ?1, ?2, ?3, 'work')", params![start, end, title]).unwrap();
            }
        }
        assert_eq!(
            blocks(&app),
            [("dst-fold".into(), 90, 90), ("dst-morning".into(), 540, 600), ("dst-late".into(), 1410, 1440)],
        );
        // The same events seen from Zurich on its Nov 1 (CET since Oct 25).
        app.clock = Clock::fixed(Ts::parse("2026-11-01T09:00:00Z").unwrap(), TimeZone::get("Europe/Zurich").unwrap());
        assert_eq!(
            blocks(&app),
            [("dst-before".into(), 450, 480), ("dst-fold".into(), 570, 615), ("dst-morning".into(), 1080, 1140)],
        );
    }
}

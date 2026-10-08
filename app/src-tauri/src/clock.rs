//! The app's clock and its time zone: the only place that turns an instant
//! into wall-clock time, or a calendar day into instants (see `crate::time`).
//! Nothing else reads the system zone or local time (a test checks), so tests
//! can pin both the zone and "now".
//!
//! DST is `jiff`'s: a wall-clock time in the repeated hour of a fall-back is
//! the first of the two, one skipped by a spring-forward is read past the gap
//! (the "compatible" rule of RFC 5545 and Temporal), and a day is however
//! long it is there (23 to 25 hours).
//!
//! `settings.fake_now` ("10:52") shifts "now" to that time today, for
//! development and screenshots.

use crate::time::{Date, Ts};
use jiff::civil::{DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, Zoned};

#[derive(Debug, Clone)]
pub struct Clock {
    offset: SignedDuration,
    tz: TimeZone,
}

impl Clock {
    /// The system zone, shifted to `fake_now` ("HH:MM") today if set. Unit
    /// tests get UTC so they don't depend on the machine's zone; tests about
    /// zones build their clock with `in_zone` or `fixed`.
    pub fn new(fake_now: &str) -> Clock {
        #[cfg(not(test))]
        let tz = TimeZone::system();
        #[cfg(test)]
        let tz = TimeZone::UTC;
        Clock::in_zone(fake_now, tz)
    }

    pub fn in_zone(fake_now: &str, tz: TimeZone) -> Clock {
        let real = Clock {
            offset: SignedDuration::ZERO,
            tz,
        };
        let offset = Time::strptime("%H:%M", fake_now)
            .map(|t| real.from_local(real.today().to_datetime(t)) - real.now())
            .unwrap_or(SignedDuration::ZERO);
        Clock { offset, ..real }
    }

    /// A clock that reads `now` at the moment it's made (tests).
    pub fn fixed(now: Ts, tz: TimeZone) -> Clock {
        Clock {
            offset: now - Ts::new(Timestamp::now()),
            tz,
        }
    }

    pub fn now(&self) -> Ts {
        Ts::new(Timestamp::now()) + self.offset
    }

    pub fn tz(&self) -> &TimeZone {
        &self.tz
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset.as_millis() as i64
    }

    /// An instant here, for showing and calendar maths.
    pub fn zoned(&self, t: Ts) -> Zoned {
        t.timestamp().to_zoned(self.tz.clone())
    }

    /// Wall-clock time of an instant here.
    pub fn local(&self, t: Ts) -> DateTime {
        self.zoned(t).datetime()
    }

    pub fn local_now(&self) -> DateTime {
        self.local(self.now())
    }

    pub fn today(&self) -> Date {
        self.local_now().date()
    }

    /// Today here by the real time, ignoring `fake_now` (for external quotas).
    pub fn real_today(&self) -> Date {
        self.day_of(Ts::new(Timestamp::now()))
    }

    /// The calendar day an instant falls on here.
    pub fn day_of(&self, t: Ts) -> Date {
        self.local(t).date()
    }

    /// The instant a wall-clock time here names (DST as above).
    pub fn from_local(&self, t: DateTime) -> Ts {
        Ts::new(
            self.tz
                .to_ambiguous_zoned(t)
                .compatible()
                .expect("wall-clock time in range")
                .timestamp(),
        )
    }

    /// A calendar day here as instants: `[start, start of the next day)`.
    pub fn day_range(&self, d: Date) -> (Ts, Ts) {
        let start = |d: Date| {
            Ts::new(
                d.to_zoned(self.tz.clone())
                    .expect("day in range")
                    .start_of_day()
                    .expect("day has a start")
                    .timestamp(),
            )
        };
        (start(d), start(d.tomorrow().expect("day in range")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn la() -> TimeZone {
        TimeZone::get("America/Los_Angeles").unwrap()
    }

    #[test]
    fn days_across_dst() {
        let c = Clock::fixed(Ts::parse("2026-11-01T12:00:00Z").unwrap(), la());
        // Fall back: Nov 1 2026 has 25 hours in Los Angeles.
        let (a, b) = c.day_range(date(2026, 11, 1));
        assert_eq!(
            (a.to_db().as_str(), b.to_db().as_str()),
            ("2026-11-01T07:00:00.000Z", "2026-11-02T08:00:00.000Z")
        );
        assert_eq!((b - a).as_hours(), 25);
        // The repeated hour reads as the first one.
        assert_eq!(
            c.from_local(date(2026, 11, 1).at(1, 30, 0, 0)).to_db(),
            "2026-11-01T08:30:00.000Z"
        );
        // Spring forward: 02:30 on Mar 8 doesn't exist; read past the gap.
        assert_eq!(
            c.local(c.from_local(date(2026, 3, 8).at(2, 30, 0, 0))).time(),
            Time::constant(3, 30, 0, 0)
        );
    }

    #[test]
    fn the_same_instant_is_a_different_day_elsewhere() {
        let t = Ts::parse("2026-10-03T05:30:00Z").unwrap();
        assert_eq!(Clock::fixed(t, la()).day_of(t), date(2026, 10, 2));
        assert_eq!(
            Clock::fixed(t, TimeZone::get("Europe/Zurich").unwrap()).day_of(t),
            date(2026, 10, 3)
        );
    }

    #[test]
    fn fake_now_is_that_time_today_in_the_zone() {
        let c = Clock::in_zone("10:52", la());
        assert_eq!(c.local_now().strftime("%H:%M").to_string(), "10:52");
    }

    /// Wall-clock time comes from `Clock` only. Protoncal hands its zone to
    /// `rrule` as chrono values, converted from the clock's; this file and the
    /// UTC migration name the old forms in prose.
    #[test]
    fn nothing_else_reads_local_time() {
        let banned = [
            "chrono::Local",
            "Local::now",
            "TimeZone::system",
            "Zoned::now",
            "localtime",
            "datetime('now",
            "date('now",
            "CURRENT_TIMESTAMP",
        ];
        let allowed = ["clock.rs", "utc_migration.rs", "protoncal.rs"];
        let mut bad = vec![];
        let mut dirs = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !allowed.iter().any(|a| p.ends_with(a)) {
                    let text = std::fs::read_to_string(&p).unwrap();
                    bad.extend(
                        banned
                            .iter()
                            .filter(|b| text.contains(*b))
                            .map(|b| format!("{}: {b}", p.display())),
                    );
                }
            }
        }
        assert!(bad.is_empty(), "local time read outside Clock: {bad:#?}");
        // Column defaults in the migrated schema.
        let conn = crate::db::open_in_memory().unwrap();
        let schema: Vec<String> = conn
            .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let bad: Vec<&String> = schema.iter().filter(|s| banned.iter().any(|b| s.contains(b))).collect();
        assert!(bad.is_empty(), "defaults that aren't UTC instants: {bad:#?}");
    }
}

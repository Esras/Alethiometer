//! The app's clock and its time zone: the only place that turns an instant
//! into wall-clock time or a calendar day into instants (see `crate::time`).
//! Nothing else uses `chrono::Local` (a test checks), so tests can pin both
//! the zone and "now".
//!
//! `settings.fake_now` ("10:52") shifts "now" to that time today, for
//! development and screenshots.

use crate::time::Ts;
use chrono::{Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, Copy)]
pub struct Clock {
    offset: Duration,
    tz: Tz,
}

impl Clock {
    /// The system's zone (UTC, with a warning, if it can't be read).
    pub fn system_tz() -> Tz {
        match iana_time_zone::get_timezone().map_err(|e| e.to_string()).and_then(|n| n.parse::<Tz>().map_err(|e| e.to_string())) {
            Ok(tz) => tz,
            Err(e) => {
                eprintln!("clock: couldn't read the system time zone ({e}); using UTC");
                Tz::UTC
            }
        }
    }

    /// The system zone, shifted to `fake_now` ("HH:MM") today if set.
    pub fn new(fake_now: &str) -> Clock {
        Clock::in_zone(fake_now, Clock::system_tz())
    }

    pub fn in_zone(fake_now: &str, tz: Tz) -> Clock {
        let real = Clock { offset: Duration::zero(), tz };
        let offset = NaiveTime::parse_from_str(fake_now, "%H:%M")
            .map(|t| real.from_local(real.today().and_time(t)) - real.now())
            .unwrap_or_else(|_| Duration::zero());
        Clock { offset, tz }
    }

    /// A clock that reads `now` at the moment it's made (tests).
    pub fn fixed(now: Ts, tz: Tz) -> Clock {
        Clock { offset: now - Ts::from_utc(Utc::now()), tz }
    }

    pub fn now(&self) -> Ts {
        Ts::from_utc(Utc::now() + self.offset)
    }

    pub fn tz(&self) -> Tz {
        self.tz
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset.num_milliseconds()
    }

    /// Wall-clock time of an instant here.
    pub fn local(&self, t: Ts) -> NaiveDateTime {
        t.in_tz(&self.tz).naive_local()
    }

    pub fn local_now(&self) -> NaiveDateTime {
        self.local(self.now())
    }

    pub fn today(&self) -> NaiveDate {
        self.local_now().date()
    }

    /// The calendar day an instant falls on here.
    pub fn day_of(&self, t: Ts) -> NaiveDate {
        self.local(t).date()
    }

    /// The instant a wall-clock time here names. In the repeated hour of a
    /// DST fall-back it's the first of the two; a time skipped by a spring
    /// forward is read as the same distance past the gap.
    pub fn from_local(&self, t: NaiveDateTime) -> Ts {
        match self.tz.from_local_datetime(&t) {
            LocalResult::Single(x) | LocalResult::Ambiguous(x, _) => Ts::from_utc(x.with_timezone(&Utc)),
            LocalResult::None => {
                // Inside a spring-forward gap: the offset before the gap, applied as if it continued.
                let before = self.tz.offset_from_utc_datetime(&(t - Duration::hours(3)));
                let off = chrono::Offset::fix(&before);
                Ts::from_utc(off.from_local_datetime(&t).single().expect("fixed offsets are unambiguous").with_timezone(&Utc))
            }
        }
    }

    /// The first instant of a calendar day here.
    pub fn day_start(&self, d: NaiveDate) -> Ts {
        self.from_local(d.and_hms_opt(0, 0, 0).expect("midnight exists"))
    }

    /// A calendar day here as instants: `[start, start of the next day)`. Not
    /// always 24 hours (DST).
    pub fn day_range(&self, d: NaiveDate) -> (Ts, Ts) {
        (self.day_start(d), self.day_start(d + Duration::days(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LA: Tz = chrono_tz::America::Los_Angeles;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn days_across_dst() {
        let c = Clock::fixed(Ts::parse("2026-11-01T12:00:00Z").unwrap(), LA);
        // Fall back: Nov 1 2026 has 25 hours in Los Angeles.
        let (a, b) = c.day_range(d("2026-11-01"));
        assert_eq!((a.to_db().as_str(), b.to_db().as_str()), ("2026-11-01T07:00:00.000Z", "2026-11-02T08:00:00.000Z"));
        assert_eq!((b - a).num_hours(), 25);
        // The repeated hour reads as the first one.
        let t = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap().and_hms_opt(1, 30, 0).unwrap();
        assert_eq!(c.from_local(t).to_db(), "2026-11-01T08:30:00.000Z");
        // Spring forward: 02:30 on Mar 8 doesn't exist; read past the gap.
        let t = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap().and_hms_opt(2, 30, 0).unwrap();
        assert_eq!(c.local(c.from_local(t)).format("%H:%M").to_string(), "03:30");
    }

    #[test]
    fn the_same_instant_is_a_different_day_elsewhere() {
        let t = Ts::parse("2026-10-03T05:30:00Z").unwrap();
        assert_eq!(Clock::fixed(t, LA).day_of(t), d("2026-10-02"));
        assert_eq!(Clock::fixed(t, chrono_tz::Europe::Zurich).day_of(t), d("2026-10-03"));
    }

    #[test]
    fn fake_now_is_that_time_today_in_the_zone() {
        let c = Clock::in_zone("10:52", LA);
        assert_eq!(c.local_now().format("%H:%M").to_string(), "10:52");
    }
}

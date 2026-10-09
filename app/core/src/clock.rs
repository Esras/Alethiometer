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
        #[cfg(not(any(test, feature = "test-support")))]
        let tz = TimeZone::system();
        #[cfg(any(test, feature = "test-support"))]
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
mod tests;

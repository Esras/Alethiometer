//! Local wall clock, with an optional fixed offset for development (`settings.fake_now`).

use chrono::{Duration, Local, NaiveDateTime, NaiveTime};

#[derive(Debug, Clone, Copy)]
pub struct Clock {
    offset: Duration,
}

impl Clock {
    pub fn new(fake_now: &str) -> Clock {
        let offset = NaiveTime::parse_from_str(fake_now, "%H:%M")
            .map(|t| Local::now().date_naive().and_time(t) - Local::now().naive_local())
            .unwrap_or_else(|_| Duration::zero());
        Clock { offset }
    }

    pub fn now(&self) -> NaiveDateTime {
        Local::now().naive_local() + self.offset
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset.num_milliseconds()
    }
}

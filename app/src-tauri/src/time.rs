//! Time in Alethiometer comes in two kinds, and nothing in between:
//!
//! - **An instant** ([`Ts`]): something happened or will happen at one moment
//!   (a message was sent, a meeting starts, a row was written). Stored, passed
//!   around and compared in UTC; turned into wall-clock time only to be shown,
//!   through the app's [`Clock`](crate::clock::Clock), the one place that knows
//!   the time zone.
//! - **A calendar day** (`chrono::NaiveDate`): a Reflect day, a Notes page, a
//!   day-only due date. Local by meaning: "Thursday" stays Thursday wherever
//!   you are. Stored as `YYYY-MM-DD`. A query for "today's" instants asks the
//!   clock for the day's UTC range.
//!
//! Instants are stored as `YYYY-MM-DDTHH:MM:SS.sssZ`: fixed width, so text
//! order is time order in SQL, and the same shape as SQLite's
//! `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`.

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use std::fmt;

/// An instant, in UTC.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ts(DateTime<Utc>);

/// SQL for "now" in the stored form, for column defaults and SQL-side stamps.
pub const SQL_NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

impl Ts {
    pub fn from_utc(t: DateTime<Utc>) -> Ts {
        // Stored to the millisecond; keep the value equal to what reads back.
        Ts(t - Duration::nanoseconds((t.timestamp_subsec_nanos() % 1_000_000) as i64))
    }

    pub fn from_unix_ms(ms: i64) -> Option<Ts> {
        DateTime::from_timestamp_millis(ms).map(Ts::from_utc)
    }

    /// Unix seconds, fractions kept to the millisecond (Slack's `ts` is "1789995600.000500").
    pub fn from_unix_secs(secs: f64) -> Option<Ts> {
        Ts::from_unix_ms((secs * 1000.0).floor() as i64)
    }

    /// Any RFC 3339 time with an offset or `Z` ("2026-10-03T10:52:00+02:00"),
    /// and the forms some APIs send instead (Jira's "+0000" without a colon).
    pub fn parse(s: &str) -> Option<Ts> {
        let s = s.trim();
        DateTime::parse_from_rfc3339(s)
            .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z"))
            .ok()
            .map(|t| Ts::from_utc(t.with_timezone(&Utc)))
    }

    pub fn utc(self) -> DateTime<Utc> {
        self.0
    }

    pub fn unix_ms(self) -> i64 {
        self.0.timestamp_millis()
    }

    /// The stored form, `YYYY-MM-DDTHH:MM:SS.sssZ`.
    pub fn to_db(self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    pub fn in_tz<Tz: TimeZone>(self, tz: &Tz) -> DateTime<Tz> {
        self.0.with_timezone(tz)
    }
}

/// The Unix epoch: only so structs with an instant can derive `Default`
/// (`sync::apply` refuses an item still carrying it).
impl Default for Ts {
    fn default() -> Ts {
        Ts(DateTime::UNIX_EPOCH)
    }
}

impl std::ops::Add<Duration> for Ts {
    type Output = Ts;
    fn add(self, d: Duration) -> Ts {
        Ts(self.0 + d)
    }
}

impl std::ops::Sub<Duration> for Ts {
    type Output = Ts;
    fn sub(self, d: Duration) -> Ts {
        Ts(self.0 - d)
    }
}

impl std::ops::Sub<Ts> for Ts {
    type Output = Duration;
    fn sub(self, o: Ts) -> Duration {
        self.0 - o.0
    }
}

impl fmt::Display for Ts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_db())
    }
}

impl fmt::Debug for Ts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ts({})", self.to_db())
    }
}

impl ToSql for Ts {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_db()))
    }
}

impl FromSql for Ts {
    fn column_result(v: ValueRef<'_>) -> FromSqlResult<Ts> {
        let s = v.as_str()?;
        Ts::parse(s).ok_or_else(|| FromSqlError::Other(format!("not a UTC instant: '{s}'").into()))
    }
}

impl serde::Serialize for Ts {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_db())
    }
}

impl<'de> serde::Deserialize<'de> for Ts {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Ts, D::Error> {
        let s = String::deserialize(d)?;
        Ts::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a UTC instant: '{s}'")))
    }
}

/// When something is due: at an instant, or some time on a calendar day (Jira's
/// `duedate`, Obsidian's `📅`), which is due by the end of that day wherever you are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    At(Ts),
    Day(NaiveDate),
}

impl Due {
    /// For ordering and "overdue": the instant itself, or the end of the day in `clock`'s zone.
    pub fn deadline(self, clock: &crate::clock::Clock) -> Ts {
        match self {
            Due::At(t) => t,
            Due::Day(d) => clock.day_start(d + Duration::days(1)) - Duration::milliseconds(1),
        }
    }

    /// The two stored columns, `(due_at, due_day)`.
    pub fn columns(due: Option<Due>) -> (Option<Ts>, Option<NaiveDate>) {
        match due {
            Some(Due::At(t)) => (Some(t), None),
            Some(Due::Day(d)) => (None, Some(d)),
            None => (None, None),
        }
    }

    pub fn from_columns(at: Option<Ts>, day: Option<NaiveDate>) -> Option<Due> {
        at.map(Due::At).or(day.map(Due::Day))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_form_is_fixed_width_utc_and_sorts_as_text() {
        let a = Ts::parse("2026-10-03T10:52:00+02:00").unwrap();
        assert_eq!(a.to_db(), "2026-10-03T08:52:00.000Z");
        let b = Ts::parse("2026-10-03T08:52:00.5Z").unwrap();
        assert!(a.to_db() < b.to_db() && a < b);
        assert_eq!(Ts::parse(&b.to_db()), Some(b), "round trip");
        assert_eq!(Ts::parse("2026-09-27T10:15:30.123+0000").unwrap().to_db(), "2026-09-27T10:15:30.123Z", "Jira's offset form");
        assert_eq!(Ts::from_unix_secs(1789995600.0005).unwrap().to_db(), "2026-09-21T11:00:00.000Z");
        assert!(Ts::parse("2026-10-03T10:52:00").is_none(), "a time without an offset isn't an instant");
    }

    #[test]
    fn sqlite_now_has_the_same_shape() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        let s: String = c.query_row(&format!("SELECT {SQL_NOW}"), [], |r| r.get(0)).unwrap();
        assert_eq!(s.len(), "2026-10-03T08:52:00.000Z".len());
        assert!(Ts::parse(&s).is_some());
    }
}

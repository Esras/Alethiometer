//! Time in Alethiometer comes in two kinds, and nothing in between:
//!
//! - **An instant** ([`Ts`]): something happened or will happen at one moment
//!   (a message was sent, a meeting starts, a row was written). Stored, passed
//!   around and compared in UTC; turned into wall-clock time only to be shown,
//!   through the app's [`Clock`](crate::clock::Clock), the one place that knows
//!   the time zone.
//! - **A calendar day** ([`Date`]): a Reflect day, a Notes page, a day-only due
//!   date. Local by meaning: "Thursday" stays Thursday wherever you are.
//!   Stored as `YYYY-MM-DD`. A query for "today's" instants asks the clock for
//!   the day's UTC range.
//!
//! The library is `jiff` (time zones, DST, parsing). `Ts` only pins how an
//! instant is stored: `YYYY-MM-DDTHH:MM:SS.sssZ`, fixed width, so text order
//! is time order in SQL, and the same shape as SQLite's
//! `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`.

pub use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use std::fmt;

/// An instant, in UTC, to the millisecond.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ts(Timestamp);

/// SQL for "now" in the stored form, for column defaults and SQL-side stamps.
pub const SQL_NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

impl Ts {
    pub fn new(t: Timestamp) -> Ts {
        // Stored to the millisecond: keep the value equal to what reads back.
        Ts(Timestamp::from_millisecond(t.as_millisecond()).expect("in range"))
    }

    pub fn timestamp(self) -> Timestamp {
        self.0
    }

    pub fn from_unix_ms(ms: i64) -> Option<Ts> {
        Timestamp::from_millisecond(ms).ok().map(Ts)
    }

    /// Unix seconds, fractions kept to the millisecond (Slack's `ts`, "1789995600.000500").
    pub fn from_unix_secs(secs: f64) -> Option<Ts> {
        Ts::from_unix_ms((secs * 1000.0).floor() as i64)
    }

    /// An RFC 3339 instant with an offset or `Z` ("2026-10-03T10:52:00+02:00"),
    /// including forms APIs send ("+0000"). A time without an offset isn't an
    /// instant and doesn't parse.
    pub fn parse(s: &str) -> Option<Ts> {
        s.trim().parse::<Timestamp>().ok().map(Ts::new)
    }

    pub fn unix_ms(self) -> i64 {
        self.0.as_millisecond()
    }

    /// The stored form, `YYYY-MM-DDTHH:MM:SS.sssZ`.
    pub fn to_db(self) -> String {
        format!("{:.3}", self.0)
    }
}

/// The Unix epoch: only so structs with an instant can derive `Default`
/// (`sync::apply` refuses an item still carrying it).
impl Default for Ts {
    fn default() -> Ts {
        Ts(Timestamp::UNIX_EPOCH)
    }
}

impl std::ops::Add<SignedDuration> for Ts {
    type Output = Ts;
    fn add(self, d: SignedDuration) -> Ts {
        Ts::new(self.0 + d)
    }
}

impl std::ops::Sub<SignedDuration> for Ts {
    type Output = Ts;
    fn sub(self, d: SignedDuration) -> Ts {
        Ts::new(self.0 - d)
    }
}

impl std::ops::Sub<Ts> for Ts {
    type Output = SignedDuration;
    fn sub(self, o: Ts) -> SignedDuration {
        self.0.duration_since(o.0)
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
    Day(Date),
}

impl Due {
    /// For ordering and "overdue": the instant itself, or the end of the day here.
    pub fn deadline(self, clock: &crate::clock::Clock) -> Ts {
        match self {
            Due::At(t) => t,
            Due::Day(d) => clock.day_range(d).1 - SignedDuration::from_millis(1),
        }
    }

    /// The two stored columns, `(due_at, due_day)`.
    pub fn columns(due: Option<Due>) -> (Option<Ts>, Option<Date>) {
        match due {
            Some(Due::At(t)) => (Some(t), None),
            Some(Due::Day(d)) => (None, Some(d)),
            None => (None, None),
        }
    }

    pub fn from_columns(at: Option<Ts>, day: Option<Date>) -> Option<Due> {
        at.map(Due::At).or(day.map(Due::Day))
    }
}

/// A fixture instant: "2026-09-28T09:00:00" (read as UTC) or any RFC 3339 time.
#[cfg(test)]
pub fn t(s: &str) -> Ts {
    Ts::parse(s).or_else(|| Ts::parse(&format!("{s}Z"))).unwrap_or_else(|| panic!("not a fixture time: '{s}'"))
}

/// Fixtures can write instants as strings (`"2026-09-28T09:00:00".into()`, read as UTC).
#[cfg(test)]
impl From<&str> for Ts {
    fn from(s: &str) -> Ts {
        t(s)
    }
}

#[cfg(test)]
impl From<String> for Ts {
    fn from(s: String) -> Ts {
        t(&s)
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
        assert_eq!(Ts::from_unix_secs(1789995600.0005).unwrap().to_db(), "2026-09-21T13:00:00.000Z");
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

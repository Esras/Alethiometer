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

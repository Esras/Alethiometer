use super::*;

#[test]
fn stored_form_is_fixed_width_utc_and_sorts_as_text() {
    let a = Ts::parse("2026-10-03T10:52:00+02:00").unwrap();
    assert_eq!(a.to_db(), "2026-10-03T08:52:00.000Z");
    let b = Ts::parse("2026-10-03T08:52:00.5Z").unwrap();
    assert!(a.to_db() < b.to_db() && a < b);
    assert_eq!(Ts::parse(&b.to_db()), Some(b), "round trip");
    assert_eq!(
        Ts::parse("2026-09-27T10:15:30.123+0000").unwrap().to_db(),
        "2026-09-27T10:15:30.123Z",
        "Jira's offset form"
    );
    assert_eq!(
        Ts::from_unix_secs(1789995600.0005).unwrap().to_db(),
        "2026-09-21T13:00:00.000Z"
    );
    assert!(
        Ts::parse("2026-10-03T10:52:00").is_none(),
        "a time without an offset isn't an instant"
    );
}

#[test]
fn sqlite_now_has_the_same_shape() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    let s: String = c.query_row(&format!("SELECT {SQL_NOW}"), [], |r| r.get(0)).unwrap();
    assert_eq!(s.len(), "2026-10-03T08:52:00.000Z".len());
    assert!(Ts::parse(&s).is_some());
}

//! Values the database maintains equal what their from-scratch queries give. Needs the sample data, which is built
//! above the store.

use alethiometer_core::clock::Clock;
use alethiometer_core::store::{over, schema};
use alethiometer_core::time::Ts;
use alethiometer_lib::seed;

/// After the kinds of writes the app makes.
#[test]
fn maintained_values_match_their_from_scratch_queries() {
    let conn = schema::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::fixed(Ts::parse("2026-09-27T10:52:00Z").unwrap(), jiff::tz::TimeZone::UTC);
    seed::seed(&over(&conn), &clock, dir.path()).unwrap();
    // Times move (a rescheduled meeting), links are made and moved, items come back.
    conn.execute_batch(
        "UPDATE items SET occurred_at = '2026-09-29T08:00:00.000Z', ended_at = '2026-09-29T09:00:00.000Z' WHERE id IN (SELECT id FROM items WHERE kind = 'event' LIMIT 3);
         UPDATE items SET ended_at = NULL WHERE id IN (SELECT id FROM items WHERE ended_at IS NOT NULL LIMIT 2);
         INSERT OR IGNORE INTO topic_links (item_id, topic_id, state) SELECT i.id, (SELECT min(id) FROM topics), 'confirmed' FROM items i LIMIT 5;
         UPDATE topic_links SET item_id = (SELECT max(id) FROM items) WHERE rowid = (SELECT max(rowid) FROM topic_links);
         UPDATE items SET seen = 0, status = 'open', owner = 'you', kind = 'task' WHERE id IN (SELECT id FROM items LIMIT 4);",
    )
    .unwrap();
    let stale: i64 = conn
        .query_row(
            "SELECT count(*) FROM topic_links tl JOIN items i ON i.id = tl.item_id WHERE tl.item_at IS NOT coalesce(i.ended_at, i.occurred_at)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stale, 0, "topic_links.item_at follows its item");
    let counts = |sql: &str| -> Vec<(i64, i64)> {
        conn.prepare(sql)
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    let links = "topic_links tl JOIN items i ON i.id = tl.item_id";
    assert_eq!(
        counts("SELECT topic_id, n FROM topic_new_counts ORDER BY topic_id"),
        counts(&format!(
            "SELECT t.id, (SELECT count(*) FROM {links} WHERE tl.topic_id = t.id AND tl.state IN ('suggested', 'confirmed') AND i.seen = 0) AS n
               FROM topics t WHERE n > 0 ORDER BY t.id"
        ))
    );
    assert_eq!(
        counts("SELECT topic_id, n FROM topic_open_counts ORDER BY topic_id"),
        counts(&format!(
            "SELECT t.id, (SELECT count(*) FROM {links} WHERE tl.topic_id = t.id AND tl.state = 'confirmed'
                           AND i.kind IN ('task', 'issue', 'email') AND i.owner IS NOT NULL AND i.status = 'open') AS n
               FROM topics t WHERE n > 0 ORDER BY t.id"
        ))
    );
}

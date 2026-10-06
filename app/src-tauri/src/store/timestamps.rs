//! Every table has `created_at` and `updated_at` (user, 2026-10-06), kept by
//! the database: a column default or an insert trigger sets both when a row
//! is stored, and an update trigger moves `updated_at` when any other column
//! really changes. Rewriting a row with the values it already has (what a sync
//! upsert does) leaves it alone, and so does a write that sets `updated_at`
//! itself.
//!
//! `ensure` runs after the last migration whenever any ran (`schema::migrate`):
//! it adds the columns a table lacks and rebuilds every table's triggers from
//! its current columns, so a later migration that adds a column or rebuilds a
//! table needs nothing extra. SQLite can't add a column whose default is the
//! current time, so an added column defaults to '' and the insert trigger
//! fills it in the same statement; a table created with the defaults
//! (`DEFAULT (strftime(...))`) never sees ''.
//!
//! Rows already there when a column is added get the best time there is:
//! an item its own time (`occurred_at`, never later than now), a table that
//! already kept `updated_at` that value as `created_at`, anything else the
//! time of the migration.
//!
//! Left out: virtual tables (full-text search, sqlite-vec) and their shadow
//! tables, which SQLite owns. A row replaced with `INSERT OR REPLACE`, or
//! deleted and stored again, is a new row with a new `created_at`.

use crate::time::SQL_NOW;
use rusqlite::Connection;

/// Ordinary tables in `main`, without SQLite's own and virtual tables' shadow tables.
pub fn tables(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let all: Vec<(String, String)> = conn
        .prepare("SELECT name, type FROM pragma_table_list WHERE schema = 'main' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    // Not every module marks its shadow tables (`type = 'shadow'`), so also
    // leave out anything named after a virtual table.
    let virtuals: Vec<&str> = all.iter().filter(|(_, t)| t == "virtual").map(|(n, _)| n.as_str()).collect();
    let mut out: Vec<String> = all
        .iter()
        .filter(|(n, t)| t == "table" && !virtuals.iter().any(|v| n.starts_with(&format!("{v}_"))))
        .map(|(n, _)| n.clone())
        .collect();
    out.sort();
    Ok(out)
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?
        .query_map([table], |r| r.get(0))?
        .collect()
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn ensure(conn: &Connection) -> anyhow::Result<()> {
    for table in tables(conn)? {
        let t = quote(&table);
        let cols = columns(conn, &table)?;
        let had_created = cols.iter().any(|c| c == "created_at");
        let had_updated = cols.iter().any(|c| c == "updated_at");
        if !had_created {
            conn.execute_batch(&format!("ALTER TABLE {t} ADD COLUMN created_at TEXT NOT NULL DEFAULT ''"))?;
            let from = if table == "items" {
                format!("CASE WHEN occurred_at < {SQL_NOW} THEN occurred_at ELSE {SQL_NOW} END")
            } else if had_updated {
                "updated_at".to_string()
            } else {
                SQL_NOW.to_string()
            };
            conn.execute_batch(&format!("UPDATE {t} SET created_at = {from}"))?;
        }
        if !had_updated {
            conn.execute_batch(&format!("ALTER TABLE {t} ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''"))?;
            conn.execute_batch(&format!("UPDATE {t} SET updated_at = created_at"))?;
        }

        let cols = columns(conn, &table)?;
        let changed: Vec<String> = cols.iter().filter(|c| *c != "updated_at").map(|c| format!("NEW.{0} IS NOT OLD.{0}", quote(c))).collect();
        let created = quote(&format!("{table}_created_at"));
        let updated = quote(&format!("{table}_updated_at"));
        conn.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS {created};
             CREATE TRIGGER {created} AFTER INSERT ON {t}
             WHEN NEW.created_at = '' OR NEW.updated_at = ''
             BEGIN
               UPDATE {t} SET created_at = CASE NEW.created_at WHEN '' THEN {SQL_NOW} ELSE NEW.created_at END,
                              updated_at = CASE NEW.updated_at WHEN '' THEN {SQL_NOW} ELSE NEW.updated_at END
                WHERE rowid = NEW.rowid;
             END;
             DROP TRIGGER IF EXISTS {updated};
             CREATE TRIGGER {updated} AFTER UPDATE ON {t}
             WHEN NEW.updated_at IS OLD.updated_at AND ({})
             BEGIN
               UPDATE {t} SET updated_at = {SQL_NOW} WHERE rowid = NEW.rowid;
             END;",
            changed.join(" OR ")
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        crate::store::schema::open_in_memory().unwrap()
    }

    #[test]
    fn every_table_has_both_times_and_triggers_over_all_its_columns() {
        let conn = db();
        let tables = tables(&conn).unwrap();
        assert!(tables.contains(&"items".to_string()) && tables.contains(&"topics".to_string()));
        assert!(!tables.iter().any(|t| t.starts_with("items_fts") || t.starts_with("vec_")), "{tables:?}");
        for t in &tables {
            let cols = columns(&conn, t).unwrap();
            assert!(cols.contains(&"created_at".into()) && cols.contains(&"updated_at".into()), "{t}: {cols:?}");
            let sql: String = conn
                .query_row("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1", [format!("{t}_updated_at")], |r| r.get(0))
                .unwrap_or_else(|_| panic!("{t} has no updated_at trigger"));
            for c in cols.iter().filter(|c| *c != "updated_at") {
                assert!(sql.contains(&format!("NEW.\"{c}\" IS NOT OLD.\"{c}\"")), "{t}'s trigger misses {c}");
            }
        }
    }

    fn times(conn: &Connection) -> (String, String) {
        conn.query_row("SELECT created_at, updated_at FROM topics WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
    }

    #[test]
    fn stored_rows_get_both_and_only_a_real_change_moves_updated_at() {
        let conn = db();
        conn.execute("INSERT INTO topics (id, name, category) VALUES (1, 'Acme', 'work')", []).unwrap();
        let (c, u) = times(&conn);
        assert!(crate::time::Ts::parse(&c).is_some(), "{c}");
        assert_eq!(c, u);

        let old = "2026-01-01T00:00:00.000Z";
        conn.execute("UPDATE topics SET created_at = ?1, updated_at = ?1", [old]).unwrap();
        assert_eq!(times(&conn), (old.into(), old.into()), "a write that sets updated_at itself wins");

        let before = conn.total_changes();
        conn.execute("UPDATE topics SET name = 'Acme' WHERE id = 1", []).unwrap();
        assert_eq!(conn.total_changes() - before, 1, "an unchanged rewrite fires nothing");
        assert_eq!(times(&conn).1, old);

        conn.execute("UPDATE topics SET name = 'Acme outage' WHERE id = 1", []).unwrap();
        let (c, u) = times(&conn);
        assert_eq!(c, old);
        assert!(u.as_str() > old, "{u}");

        // A NULL to a value is a change too.
        conn.execute("UPDATE topics SET updated_at = ?1", [old]).unwrap();
        conn.execute("UPDATE topics SET obsidian_path = 'Topics/Acme.md' WHERE id = 1", []).unwrap();
        assert!(times(&conn).1.as_str() > old);
    }

    #[test]
    fn explicit_times_on_insert_are_kept() {
        let conn = db();
        conn.execute("INSERT INTO topics (id, name, category, created_at, updated_at) VALUES (1, 'A', 'work', '2026-02-01T00:00:00.000Z', '2026-03-01T00:00:00.000Z')", []).unwrap();
        assert_eq!(times(&conn), ("2026-02-01T00:00:00.000Z".into(), "2026-03-01T00:00:00.000Z".into()));
    }

    #[test]
    fn existing_rows_get_the_best_time_there_is() {
        let conn = db();
        // As before this change: no times on items or topics.
        conn.execute_batch(
            "DROP TRIGGER items_created_at; DROP TRIGGER items_updated_at;
             ALTER TABLE items DROP COLUMN created_at; ALTER TABLE items DROP COLUMN updated_at;
             DROP TRIGGER topics_created_at; DROP TRIGGER topics_updated_at;
             ALTER TABLE topics DROP COLUMN created_at; ALTER TABLE topics DROP COLUMN updated_at;
             INSERT INTO sources (id, kind, tag, name) VALUES (1, 'jira', 'work', 'Jira');
             INSERT INTO items (id, source_id, external_id, kind, title, occurred_at) VALUES
               (1, 1, 'a', 'issue', 'Old', '2026-09-01T10:00:00.000Z'),
               (2, 1, 'b', 'issue', 'Future', '2999-01-01T00:00:00.000Z');
             INSERT INTO topics (id, name, category) VALUES (1, 'T', 'work');",
        )
        .unwrap();
        ensure(&conn).unwrap();
        let item = |id: i64| conn.query_row("SELECT created_at, updated_at FROM items WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap();
        assert_eq!(item(1), ("2026-09-01T10:00:00.000Z".into(), "2026-09-01T10:00:00.000Z".into()));
        assert!(item(2).0.as_str() < "2999", "never later than now");
        let (c, u) = times(&conn);
        assert_eq!(c, u);
        assert!(crate::time::Ts::parse(&c).is_some());
        // Running it again changes nothing.
        ensure(&conn).unwrap();
        assert_eq!(item(1).0, "2026-09-01T10:00:00.000Z");
    }
}

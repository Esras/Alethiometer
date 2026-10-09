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
    let virtuals: Vec<&str> = all
        .iter()
        .filter(|(_, t)| t == "virtual")
        .map(|(n, _)| n.as_str())
        .collect();
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
            conn.execute_batch(&format!(
                "ALTER TABLE {t} ADD COLUMN created_at TEXT NOT NULL DEFAULT ''"
            ))?;
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
            conn.execute_batch(&format!(
                "ALTER TABLE {t} ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''"
            ))?;
            conn.execute_batch(&format!("UPDATE {t} SET updated_at = created_at"))?;
        }

        let cols = columns(conn, &table)?;
        let changed: Vec<String> = cols
            .iter()
            .filter(|c| *c != "updated_at")
            .map(|c| format!("NEW.{0} IS NOT OLD.{0}", quote(c)))
            .collect();
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
mod tests;

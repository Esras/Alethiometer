//! Topic categories (label + color). Keys are stable; `work` (the default) and
//! `life` (Life busy blocks) are built in and can't be removed.

use super::arg_str;
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::settings::Category;
use crate::state::App;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const BUILT_IN: [&str; 2] = ["work", "life"];

#[derive(Serialize)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub color: String,
}

pub fn seed_if_empty(conn: &Connection, from: &BTreeMap<String, Category>) -> Result<()> {
    let n: i64 = conn.query_row("SELECT count(*) FROM categories", [], |r| r.get(0))?;
    if n == 0 {
        for (i, (key, c)) in from.iter().enumerate() {
            conn.execute("INSERT INTO categories (key, label, color, position) VALUES (?1, ?2, ?3, ?4)", params![key, c.label, c.color, i as i64])?;
        }
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare("SELECT key, label, color FROM categories ORDER BY position, label")?;
    let rows = stmt.query_map([], |r| Ok(Row { key: r.get(0)?, label: r.get(1)?, color: r.get(2)? }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn map(conn: &Connection) -> Result<BTreeMap<String, Category>> {
    Ok(list(conn)?.into_iter().map(|r| (r.key, Category { label: r.label, color: r.color })).collect())
}

pub fn exists(conn: &Connection, key: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM categories WHERE key = ?1)", [key], |r| r.get(0))?)
}

fn color(s: &str) -> Result<String> {
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return Err(msg(format!("color must look like #a1b2c3, not '{s}'")));
    }
    Ok(s.to_ascii_lowercase())
}

fn label(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(msg("category name can't be empty"));
    }
    Ok(s.to_string())
}

fn create(conn: &Connection, name: &str, c: &str) -> Result<Value> {
    let (name, c) = (label(name)?, color(c)?);
    let base: String = name.to_lowercase().chars().map(|ch| if ch.is_alphanumeric() { ch } else { '-' }).collect();
    let base = base.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let base = if base.is_empty() { "category".to_string() } else { base };
    let mut key = base.clone();
    let mut n = 2;
    while exists(conn, &key)? {
        key = format!("{base}-{n}");
        n += 1;
    }
    conn.execute(
        "INSERT INTO categories (key, label, color, position) VALUES (?1, ?2, ?3, (SELECT coalesce(max(position), 0) + 1 FROM categories))",
        params![key, name, c],
    )?;
    Ok(json!({ "key": key }))
}

fn update(conn: &Connection, key: &str, name: Option<&str>, c: Option<&str>) -> Result<Value> {
    if !exists(conn, key)? {
        return Err(msg(format!("unknown category '{key}'")));
    }
    if let Some(n) = name {
        conn.execute("UPDATE categories SET label = ?2 WHERE key = ?1", params![key, label(n)?])?;
    }
    if let Some(c) = c {
        conn.execute("UPDATE categories SET color = ?2 WHERE key = ?1", params![key, color(c)?])?;
    }
    Ok(json!({ "ok": true }))
}

fn delete(conn: &Connection, key: &str) -> Result<Value> {
    if BUILT_IN.contains(&key) {
        return Err(msg(format!("'{key}' is built in and can't be removed")));
    }
    let (topics, events): (i64, i64) = conn.query_row(
        "SELECT (SELECT count(*) FROM topics WHERE category = ?1), (SELECT count(*) FROM events WHERE category = ?1)",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if topics + events > 0 {
        return Err(msg(format!("still used by {topics} topic(s) and {events} calendar block(s); move them first")));
    }
    conn.execute("DELETE FROM categories WHERE key = ?1", [key])?;
    Ok(json!({ "ok": true }))
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let conn = app.db();
    match name {
        "list" => Ok(json!(list(&conn)?)),
        "create" => create(&conn, &arg_str(args, "label")?, &arg_str(args, "color")?),
        "update" => {
            let s = |k| args.get(k).and_then(Value::as_str);
            update(&conn, &arg_str(args, "key")?, s("label"), s("color"))
        }
        "delete" => delete(&conn, &arg_str(args, "key")?),
        _ => Err(unknown(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_and_guarded_delete() {
        let conn = crate::db::open_in_memory().unwrap();
        seed_if_empty(&conn, &crate::settings::Settings::default().categories).unwrap();
        let key = create(&conn, "Leisure & fun", "#00AAFF").unwrap()["key"].as_str().unwrap().to_string();
        assert_eq!(key, "leisure-fun");
        assert_eq!(create(&conn, "Leisure fun", "#00aaff").unwrap()["key"], "leisure-fun-2");
        assert!(create(&conn, "Bad", "blue").is_err());
        update(&conn, &key, Some("Leisure"), Some("#112233")).unwrap();
        let m = map(&conn).unwrap();
        assert_eq!((m[&key].label.as_str(), m[&key].color.as_str()), ("Leisure", "#112233"));
        conn.execute("INSERT INTO topics (name, category) VALUES ('Climbing', ?1)", [&key]).unwrap();
        assert!(delete(&conn, &key).is_err());
        assert!(delete(&conn, "work").is_err());
        conn.execute("DELETE FROM topics", []).unwrap();
        delete(&conn, &key).unwrap();
        assert!(!exists(&conn, &key).unwrap());
    }
}

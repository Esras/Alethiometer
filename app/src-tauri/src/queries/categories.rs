//! Topic categories (label + color). `settings.json` is the source of truth; the table mirrors it
//! and every change here is written back. Keys are stable; `work` (the default) and `life`
//! (Life busy blocks) are built in and can't be removed.

use super::arg_str;
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::settings::{Categories, Category, Settings};
use crate::state::App;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const BUILT_IN: [&str; 2] = ["work", "life"];

pub fn load(conn: &Connection, from: &Categories) -> Result<bool> {
    let rows = match from {
        Categories::Legacy(m) => {
            if list(conn)?.is_empty() {
                let rows: Vec<Category> = m.iter().map(|(k, c)| Category { key: k.clone(), label: c.label.clone(), color: c.color.clone() }).collect();
                replace(conn, &rows)?;
            }
            return Ok(true);
        }
        Categories::List(rows) => rows,
    };
    let mut keep: Vec<Category> = vec![];
    for c in rows {
        let key = c.key.trim();
        if key.is_empty() || keep.iter().any(|k| k.key == key) {
            return Err(msg(format!("settings.json categories: key '{key}' is empty or repeated")));
        }
        keep.push(Category { key: key.into(), label: label(&c.label)?, color: color(&c.color)? });
    }
    let n = keep.len();
    let defaults = match Settings::default().categories {
        Categories::List(d) => d,
        Categories::Legacy(_) => vec![],
    };
    for c in list(conn)?.into_iter().chain(defaults) {
        let (topics, events) = uses(conn, &c.key)?;
        if !keep.iter().any(|k| k.key == c.key) && (BUILT_IN.contains(&c.key.as_str()) || topics + events > 0) {
            eprintln!("settings.json: kept category '{}' (built in or still used)", c.key);
            keep.push(c);
        }
    }
    replace(conn, &keep)?;
    Ok(keep.len() != n)
}

fn replace(conn: &Connection, rows: &[Category]) -> Result<()> {
    conn.execute("DELETE FROM categories", [])?;
    for (i, c) in rows.iter().enumerate() {
        conn.execute("INSERT INTO categories (key, label, color, position) VALUES (?1, ?2, ?3, ?4)", params![c.key, c.label, c.color, i as i64])?;
    }
    Ok(())
}

pub fn save(conn: &Connection, path: &Path) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        r => r.map_err(|e| msg(format!("couldn't read {}: {e}", path.display())))?,
    };
    let mut v: Value = serde_json::from_str(&text).map_err(|e| msg(format!("couldn't parse {}: {e}", path.display())))?;
    v["categories"] = json!(list(conn)?);
    let tmp = path.with_extension("json.tmp");
    let out = serde_json::to_string_pretty(&v).map_err(|e| msg(e.to_string()))?;
    std::fs::write(&tmp, out + "\n")
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| msg(format!("couldn't write {}: {e}", path.display())))
}

pub fn list(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare("SELECT key, label, color FROM categories ORDER BY position, label")?;
    let rows = stmt.query_map([], |r| Ok(Category { key: r.get(0)?, label: r.get(1)?, color: r.get(2)? }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn map(conn: &Connection) -> Result<BTreeMap<String, Value>> {
    Ok(list(conn)?.into_iter().map(|r| (r.key, json!({ "label": r.label, "color": r.color }))).collect())
}

pub fn exists(conn: &Connection, key: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM categories WHERE key = ?1)", [key], |r| r.get(0))?)
}

fn uses(conn: &Connection, key: &str) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT (SELECT count(*) FROM topics WHERE category = ?1), (SELECT count(*) FROM events WHERE category = ?1)",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
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
    let (topics, events) = uses(conn, key)?;
    if topics + events > 0 {
        return Err(msg(format!("still used by {topics} topic(s) and {events} calendar block(s); move them first")));
    }
    conn.execute("DELETE FROM categories WHERE key = ?1", [key])?;
    Ok(json!({ "ok": true }))
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let conn = app.db();
    if name == "list" {
        return Ok(json!(list(&conn)?));
    }
    let tx = conn.unchecked_transaction()?;
    let out = match name {
        "create" => create(&tx, &arg_str(args, "label")?, &arg_str(args, "color")?),
        "update" => {
            let s = |k| args.get(k).and_then(Value::as_str);
            update(&tx, &arg_str(args, "key")?, s("label"), s("color"))
        }
        "delete" => delete(&tx, &arg_str(args, "key")?),
        _ => Err(unknown(name)),
    }?;
    save(&tx, &app.data_dir.join("settings.json"))?;
    tx.commit()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_and_guarded_delete() {
        let conn = crate::db::open_in_memory().unwrap();
        load(&conn, &crate::settings::Settings::default().categories).unwrap();
        let key = create(&conn, "Leisure & fun", "#00AAFF").unwrap()["key"].as_str().unwrap().to_string();
        assert_eq!(key, "leisure-fun");
        assert_eq!(create(&conn, "Leisure fun", "#00aaff").unwrap()["key"], "leisure-fun-2");
        assert!(create(&conn, "Bad", "blue").is_err());
        update(&conn, &key, Some("Leisure"), Some("#112233")).unwrap();
        let m = map(&conn).unwrap();
        assert_eq!(m[&key], json!({ "label": "Leisure", "color": "#112233" }));
        conn.execute("INSERT INTO topics (name, category) VALUES ('Climbing', ?1)", [&key]).unwrap();
        assert!(delete(&conn, &key).is_err());
        assert!(delete(&conn, "work").is_err());
        conn.execute("DELETE FROM topics", []).unwrap();
        delete(&conn, &key).unwrap();
        assert!(!exists(&conn, &key).unwrap());
    }

    #[test]
    fn file_is_the_source_of_truth() {
        let conn = crate::db::open_in_memory().unwrap();
        let cat = |k: &str, l: &str| Category { key: k.into(), label: l.into(), color: "#112233".into() };
        replace(&conn, &[cat("work", "Work"), cat("vendors", "Vendors")]).unwrap();
        let legacy: Categories = serde_json::from_value(json!({ "work": { "label": "Old", "color": "#000000" } })).unwrap();
        assert!(load(&conn, &legacy).unwrap());
        assert_eq!(list(&conn).unwrap().len(), 2);

        conn.execute("INSERT INTO topics (name, category) VALUES ('Acme', 'vendors')", []).unwrap();
        let file: Categories = serde_json::from_value(json!([{ "key": "jobs", "label": "Jobs", "color": "#B8BB26" }])).unwrap();
        assert!(load(&conn, &file).unwrap());
        let keys: Vec<String> = list(&conn).unwrap().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["jobs", "work", "vendors", "life"]);
        assert!(load(&conn, &serde_json::from_value(json!([{ "key": "a", "label": "A", "color": "blue" }])).unwrap()).is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"sources": [], "categories": {}, "demo_data": false}"#).unwrap();
        save(&conn, &path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["sources", "categories", "demo_data"]);
        assert_eq!(v["categories"][0], json!({ "key": "jobs", "label": "Jobs", "color": "#b8bb26" }));
    }
}

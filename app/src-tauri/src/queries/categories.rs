//! Topic categories (label + color). `settings.json` is the source of truth; the table mirrors it
//! and every change here is written back. Keys are stable; `work` (the default) and `life`
//! (Life busy blocks) are built in and can't be removed.

use super::arg_str;
use crate::api::{unknown, Route};
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::settings::{Categories, Category, Settings};
use crate::state::App;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const BUILT_IN: [&str; 2] = ["work", "life"];
pub const UNCATEGORIZED: &str = "";

/// The categories in settings.json, cleaned up, or why they can't be used.
/// Part of `Settings::validate`, so a bad list never gets as far as `load`.
pub fn check(rows: &[Category]) -> Result<Vec<Category>> {
    let mut keep: Vec<Category> = vec![];
    for c in rows {
        let key = c.key.trim();
        if key.is_empty() || keep.iter().any(|k| k.key == key) {
            return Err(msg(format!(
                "settings.json categories: key '{key}' is empty or repeated"
            )));
        }
        keep.push(Category {
            key: key.into(),
            label: label(&c.label)?,
            color: color(&c.color)?,
        });
    }
    Ok(keep)
}

/// Mirror settings.json's categories into the table. True when the file
/// should be written back (`save`): the old map form was migrated, or a
/// category it dropped is built in or still in use and was kept.
pub fn load(uow: &dyn UnitOfWork, from: &Categories) -> Result<bool> {
    let rows = match from {
        Categories::Legacy(m) => {
            if uow.reads().categories().list()?.is_empty() {
                let rows: Vec<Category> = m
                    .iter()
                    .map(|(k, c)| Category {
                        key: k.clone(),
                        label: c.label.clone(),
                        color: c.color.clone(),
                    })
                    .collect();
                uow.categories().replace(&rows)?;
            }
            return Ok(true);
        }
        Categories::List(rows) => rows,
    };
    let mut keep = check(rows)?;
    let n = keep.len();
    let defaults = match Settings::default().categories {
        Categories::List(d) => d,
        Categories::Legacy(_) => vec![],
    };
    for c in uow.reads().categories().list()?.into_iter().chain(defaults) {
        let (topics, events) = uow.reads().categories().uses(&c.key)?;
        if !keep.iter().any(|k| k.key == c.key) && (BUILT_IN.contains(&c.key.as_str()) || topics + events > 0) {
            eprintln!("settings.json: kept category '{}' (built in or still used)", c.key);
            keep.push(c);
        }
    }
    uow.categories().replace(&keep)?;
    Ok(keep.len() != n)
}

/// Write the table's categories into settings.json (read after the snapshot closes).
pub fn save(app: &App, path: &Path) -> Result<()> {
    let rows = app.store().read(|r| r.categories().list())?;
    write(&rows, path)
}

fn write(rows: &[Category], path: &Path) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        r => r.map_err(|e| msg(format!("couldn't read {}: {e}", path.display())))?,
    };
    let mut v: Value =
        serde_json::from_str(&text).map_err(|e| msg(format!("couldn't parse {}: {e}", path.display())))?;
    v["categories"] = json!(rows);
    crate::settings::live::write_file(path, &v)
}

pub fn map(r: &dyn Reads) -> Result<BTreeMap<String, Value>> {
    Ok(r.categories()
        .list()?
        .into_iter()
        .map(|c| (c.key, json!({ "label": c.label, "color": c.color })))
        .collect())
}

pub fn exists(r: &dyn Reads, key: &str) -> Result<bool> {
    Ok(r.categories().list()?.iter().any(|c| c.key == key))
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

fn create(uow: &dyn UnitOfWork, name: &str, c: &str) -> Result<Value> {
    let (name, c) = (label(name)?, color(c)?);
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect();
    let base = base.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let base = if base.is_empty() { "category".to_string() } else { base };
    let mut key = base.clone();
    let mut n = 2;
    while exists(uow.reads(), &key)? {
        key = format!("{base}-{n}");
        n += 1;
    }
    uow.categories().add(&Category {
        key: key.clone(),
        label: name,
        color: c,
    })?;
    Ok(json!({ "key": key }))
}

fn update(uow: &dyn UnitOfWork, key: &str, name: Option<&str>, c: Option<&str>) -> Result<Value> {
    let name = name.map(label).transpose()?;
    let c = c.map(color).transpose()?;
    if !uow.categories().update(key, name.as_deref(), c.as_deref())? {
        return Err(msg(format!("unknown category '{key}'")));
    }
    Ok(json!({ "ok": true }))
}

fn delete(uow: &dyn UnitOfWork, key: &str) -> Result<Value> {
    if BUILT_IN.contains(&key) {
        return Err(msg(format!("'{key}' is built in and can't be removed")));
    }
    let (topics, events) = uow.reads().categories().uses(key)?;
    if topics + events > 0 {
        return Err(msg(format!(
            "still used by {topics} topic(s) and {events} calendar block(s); move them first"
        )));
    }
    uow.categories().delete(key)?;
    Ok(json!({ "ok": true }))
}

pub fn route(name: &str) -> Option<Route> {
    match name {
        "list" => Some(Route::Read(|r, _, _| Ok(json!(r.categories().list()?)))),
        _ => None,
    }
}

/// A change is made in the table, then written to settings.json (the source
/// of truth). If the file can't be written, the table is put back.
pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let before = app.store().read(|r| r.categories().list())?;
    let out = app.store().write(|uow| match name {
        "create" => create(uow, &arg_str(args, "label")?, &arg_str(args, "color")?),
        "update" => {
            let s = |k| args.get(k).and_then(Value::as_str);
            update(uow, &arg_str(args, "key")?, s("label"), s("color"))
        }
        "delete" => delete(uow, &arg_str(args, "key")?),
        _ => Err(unknown(name)),
    })?;
    if let Err(e) = save(app, &app.settings_path()) {
        app.store().write(|uow| uow.categories().replace(&before))?;
        return Err(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_and_guarded_delete() {
        let conn = crate::db::open_in_memory().unwrap();
        let sql = crate::store::over(&conn);
        let uow: &dyn UnitOfWork = &sql;
        load(uow, &crate::settings::Settings::default().categories).unwrap();
        let key = create(uow, "Leisure & fun", "#00AAFF").unwrap()["key"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(key, "leisure-fun");
        assert_eq!(create(uow, "Leisure fun", "#00aaff").unwrap()["key"], "leisure-fun-2");
        assert!(create(uow, "Bad", "blue").is_err());
        update(uow, &key, Some("Leisure"), Some("#112233")).unwrap();
        let m = map(uow.reads()).unwrap();
        assert_eq!(m[&key], json!({ "label": "Leisure", "color": "#112233" }));
        conn.execute("INSERT INTO topics (name, category) VALUES ('Climbing', ?1)", [&key])
            .unwrap();
        assert!(delete(uow, &key).is_err());
        assert!(delete(uow, "work").is_err());
        conn.execute("DELETE FROM topics", []).unwrap();
        delete(uow, &key).unwrap();
        assert!(!exists(uow.reads(), &key).unwrap());
    }

    #[test]
    fn file_is_the_source_of_truth() {
        let conn = crate::db::open_in_memory().unwrap();
        let sql = crate::store::over(&conn);
        let uow: &dyn UnitOfWork = &sql;
        let list = || uow.reads().categories().list();
        let cat = |k: &str, l: &str| Category {
            key: k.into(),
            label: l.into(),
            color: "#112233".into(),
        };
        uow.categories()
            .replace(&[cat("work", "Work"), cat("vendors", "Vendors")])
            .unwrap();
        let legacy: Categories =
            serde_json::from_value(json!({ "work": { "label": "Old", "color": "#000000" } })).unwrap();
        assert!(load(uow, &legacy).unwrap());
        assert_eq!(list().unwrap().len(), 2);

        conn.execute("INSERT INTO topics (name, category) VALUES ('Acme', 'vendors')", [])
            .unwrap();
        let file: Categories =
            serde_json::from_value(json!([{ "key": "jobs", "label": "Jobs", "color": "#B8BB26" }])).unwrap();
        assert!(load(uow, &file).unwrap());
        let keys: Vec<String> = list().unwrap().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["jobs", "work", "vendors", "life"]);
        assert!(load(
            uow,
            &serde_json::from_value(json!([{ "key": "a", "label": "A", "color": "blue" }])).unwrap()
        )
        .is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"sources": [], "categories": {}, "demo_data": false}"#).unwrap();
        write(&list().unwrap(), &path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["sources", "categories", "demo_data"]
        );
        assert_eq!(
            v["categories"][0],
            json!({ "key": "jobs", "label": "Jobs", "color": "#b8bb26" })
        );
    }
}

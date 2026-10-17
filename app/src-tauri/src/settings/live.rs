//! Live settings: every change writes `settings.json`, then the app re-reads
//! the file and applies what changed. Hand edits are picked up the same way
//! (a file watcher). Nothing waits for a restart: `ui_scale` goes through the
//! webview zoom, a changed source restarts only that connector, and the few
//! settings that can't apply live (the vault path, the fake clock) restart the
//! app itself and say so.
//!
//! The Settings modal shows the outcome in its header (`State.message`):
//! "Saved · live", "Saved · Slack reconnected", "Saved · restarting".

use super::schema::path_label;
use super::{sources, Settings, SourceConfig};
use crate::error::{msg, Result};
use crate::state::App;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// What the Settings header shows. `rev` moves on every applied change, so
/// views can reload what depends on settings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct State {
    pub rev: u64,
    pub message: String,
    /// The file on disk doesn't parse or validate; the app keeps the last good settings.
    pub error: Option<String>,
}

/// One edit, addressed by path segments (a prices key can contain dots).
#[derive(Debug, Clone)]
pub enum Edit {
    Set(Vec<Value>, Value),
    /// Remove an array element or an object key.
    Remove(Vec<Value>),
    /// Append to an array.
    Push(Vec<Value>, Value),
}

impl Edit {
    pub fn parse(v: &Value) -> Result<Edit> {
        let path = v
            .get("path")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| msg("an edit needs 'path' as an array"))?;
        if path.is_empty() {
            return Err(msg("an edit needs a non-empty path"));
        }
        Ok(match v.get("op").and_then(Value::as_str).unwrap_or("set") {
            "set" => Edit::Set(path, v.get("value").cloned().unwrap_or(Value::Null)),
            "remove" => Edit::Remove(path),
            "push" => Edit::Push(path, v.get("value").cloned().unwrap_or(Value::Null)),
            other => return Err(msg(format!("unknown edit op '{other}'"))),
        })
    }

    fn path(&self) -> &[Value] {
        match self {
            Edit::Set(p, _) | Edit::Remove(p) | Edit::Push(p, _) => p,
        }
    }
}

fn slot<'a>(root: &'a mut Value, path: &[Value], create: bool) -> Result<&'a mut Value> {
    let mut cur = root;
    for seg in path {
        cur = match seg {
            Value::String(k) => {
                if !cur.is_object() {
                    if !create || !cur.is_null() {
                        return Err(msg(format!("{} isn't an object", path_label(path))));
                    }
                    *cur = json!({});
                }
                let obj = cur.as_object_mut().expect("object");
                if !obj.contains_key(k) {
                    if !create {
                        return Err(msg(format!("no setting {}", path_label(path))));
                    }
                    obj.insert(k.clone(), Value::Null);
                }
                obj.get_mut(k).expect("inserted")
            }
            Value::Number(n) => {
                let i = n.as_u64().ok_or_else(|| msg("bad index"))? as usize;
                let arr = cur
                    .as_array_mut()
                    .ok_or_else(|| msg(format!("{} isn't a list", path_label(path))))?;
                arr.get_mut(i)
                    .ok_or_else(|| msg(format!("{} is out of range", path_label(path))))?
            }
            _ => return Err(msg("path segments are keys or indexes")),
        };
    }
    Ok(cur)
}

fn apply_edit(root: &mut Value, e: &Edit) -> Result<()> {
    match e {
        Edit::Set(p, v) => *slot(root, p, true)? = v.clone(),
        Edit::Push(p, v) => {
            let target = slot(root, p, true)?;
            if target.is_null() {
                *target = json!([]);
            }
            target
                .as_array_mut()
                .ok_or_else(|| msg(format!("{} isn't a list", path_label(p))))?
                .push(v.clone());
        }
        Edit::Remove(p) => {
            let (last, parent) = p.split_last().expect("non-empty");
            let parent = slot(root, parent, false)?;
            match (parent, last) {
                (Value::Array(a), Value::Number(n)) => {
                    let i = n.as_u64().unwrap_or(u64::MAX) as usize;
                    if i >= a.len() {
                        return Err(msg(format!("{} is out of range", path_label(p))));
                    }
                    a.remove(i);
                }
                (Value::Object(o), Value::String(k)) => {
                    o.shift_remove(k);
                }
                _ => return Err(msg(format!("can't remove {}", path_label(p)))),
            }
        }
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).map_err(|e| msg(format!("couldn't read {}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| msg(format!("settings.json doesn't parse: {e}")))
}

/// Atomically (`fsx::write_atomic`), so a reader never sees half a file.
pub fn write_file(path: &Path, v: &Value) -> Result<()> {
    let out = serde_json::to_string_pretty(v).map_err(|e| msg(e.to_string()))? + "\n";
    crate::fsx::write_atomic(path, out.as_bytes(), None)
        .map_err(|e| msg(format!("couldn't write {}: {e}", path.display())))
}

/// Parse and check a whole settings document.
pub fn parse(v: &Value) -> Result<Settings> {
    let s: Settings = serde_json::from_value(v.clone()).map_err(|e| msg(format!("settings.json: {e}")))?;
    s.validate().map_err(|e| msg(e.to_string()))?;
    Ok(s)
}

/// One-time cleanups of the file, run before it's first read:
/// `life_calendars` moves into the protoncal source's `urls`.
pub fn migrate_file(path: &Path) -> Result<()> {
    let mut v = read_file(path)?;
    let links: Vec<Value> = v
        .get("life_calendars")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|u| u.as_str().is_some_and(|s| !s.trim().is_empty()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if links.is_empty() {
        return Ok(());
    }
    let sources = v
        .as_object_mut()
        .ok_or_else(|| msg("settings.json isn't an object"))?
        .entry("sources")
        .or_insert(json!([]));
    let list = sources
        .as_array_mut()
        .ok_or_else(|| msg("settings.json: sources isn't a list"))?;
    match list.iter_mut().find(|s| s.get("kind").and_then(Value::as_str) == Some("protoncal")) {
        Some(src) => {
            if !src.get("config").is_some_and(Value::is_object) {
                src["config"] = json!({});
            }
            let has = src["config"].get("urls").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
            if !has {
                src["config"]["urls"] = Value::Array(links.clone());
            }
        }
        None => list.push(json!({ "kind": "protoncal", "tag": "personal", "name": "Life", "enabled": true, "poll_secs": 900, "config": { "urls": links } })),
    }
    v.as_object_mut().expect("object").shift_remove("life_calendars");
    eprintln!("settings.json: moved life_calendars into the protoncal source");
    write_file(path, &v)
}

/// Load from `path`, writing defaults there first if the file doesn't exist.
fn load_or_init(path: &Path) -> anyhow::Result<Settings> {
    if !path.exists() {
        let s = Settings::default();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&s)?)?;
        return Ok(s);
    }
    migrate_file(path)?;
    let text = std::fs::read_to_string(path)?;
    // Not validated here: a bad hand edit mustn't stop the app opening.
    // `startup` checks it and shows the problem in Settings.
    Ok(serde_json::from_str(&text)?)
}

/// Startup: record a validation problem with the file the app opened with
/// (it still runs on it; Settings shows the error).
/// Where the last settings that parsed and validated are kept.
fn last_good_path(path: &Path) -> std::path::PathBuf {
    path.with_file_name("settings.last-good.json")
}

fn keep_last_good(path: &Path, s: &Settings) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        if let Err(e) = crate::fsx::write_atomic(&last_good_path(path), text.as_bytes(), None) {
            eprintln!("settings: couldn't keep a last good copy: {e}");
        }
    }
}

/// The settings to open the app with, and why settings.json isn't them.
///
/// A bad hand edit (doesn't parse, wrong type, fails `validate`) mustn't stop
/// the app opening, nor be overwritten: the app runs on the last settings
/// that were good (`settings.last-good.json`), or on defaults without sample
/// data when there are none, and Settings shows the problem until the file
/// is fixed (the watcher picks the fix up).
pub fn load_at_startup(path: &Path) -> anyhow::Result<(Settings, Option<String>)> {
    let read = load_or_init(path).and_then(|s| {
        s.validate()?;
        Ok(s)
    });
    match read {
        Ok(s) => {
            keep_last_good(path, &s);
            Ok((s, None))
        }
        Err(e) => {
            let last = std::fs::read_to_string(last_good_path(path))
                .ok()
                .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
                .filter(|s| s.validate().is_ok());
            let why = match &last {
                Some(_) => format!("settings.json: {e}. Running on the last good settings until it's fixed."),
                None => format!("settings.json: {e}. Running on defaults until it's fixed."),
            };
            eprintln!("{why}");
            Ok((
                last.unwrap_or_else(|| Settings {
                    demo_data: false,
                    ..Default::default()
                }),
                Some(why),
            ))
        }
    }
}

pub fn startup(app: &App, problem: Option<String>) {
    if problem.is_some() {
        app.settings_state().error = problem;
    }
}

/// Apply edits: write the file, then reload. Returns the new state.
pub fn patch(app: &App, edits: &[Edit]) -> Result<State> {
    patch_with(app, |_| Ok(edits.to_vec()))
}

/// Add `entry` to a Slack or Teams source's `muted` list (Incoming's ignore
/// suggestions), the source found by kind and name in the file as it is now.
pub fn mute(app: &App, kind: &str, name: &str, entry: &str) -> Result<State> {
    if !crate::settings::sources::is_chat(kind) {
        return Err(msg(format!("a {kind} source has no muted list")));
    }
    patch_with(app, |v| {
        let sources = v.get("sources").and_then(Value::as_array);
        let i = sources
            .and_then(|a| {
                a.iter().position(|s| {
                    s.get("kind").and_then(Value::as_str) == Some(kind)
                        && s.get("name").and_then(Value::as_str) == Some(name)
                })
            })
            .ok_or_else(|| msg(format!("no {kind} source named '{name}'")))?;
        let muted = v
            .pointer(&format!("/sources/{i}/config/muted"))
            .and_then(Value::as_array);
        if muted.is_some_and(|m| m.iter().any(|m| m.as_str() == Some(entry))) {
            return Ok(vec![]);
        }
        Ok(vec![Edit::Push(
            vec!["sources".into(), json!(i), "config".into(), "muted".into()],
            json!(entry),
        )])
    })
}

/// Apply the edits `edits` makes from the file as it is, under the write lock.
fn patch_with(app: &App, edits: impl FnOnce(&Value) -> Result<Vec<Edit>>) -> Result<State> {
    let path = app.settings_path();
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = read_file(&path)?;
    let edits = edits(&v)?;
    let edits = edits.as_slice();
    let before = v.clone();
    let by_index = |e: &Edit| {
        e.path()
            .get(..3)
            .is_some_and(|p| p[0] == "agents" && p[1] == "harnesses" && p[2].is_number())
    };
    if edits.iter().any(by_index) && v.pointer("/agents/harnesses").is_none() {
        apply_edit(
            &mut v,
            &Edit::Set(
                vec!["agents".into(), "harnesses".into()],
                json!(app.settings().agents.harnesses),
            ),
        )?;
    }
    for e in edits {
        if e.path().first().and_then(Value::as_str) == Some("ai")
            && e.path().get(1).and_then(Value::as_str) == Some("allowed_providers")
        {
            return Err(msg(
                "ai.allowed_providers is the safety gate for work data: change it in settings.json by hand",
            ));
        }
        apply_edit(&mut v, e)?;
    }
    let new = parse(&v)?;
    rename_sources(app, &before, &new)?;
    write_file(&path, &v)?;
    Ok(apply(app, new))
}

/// A source renamed in place keeps its database row (cursor, items, links).
fn rename_sources(app: &App, before: &Value, new: &Settings) -> Result<()> {
    let old: Vec<(String, String)> = before
        .get("sources")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        s.get("kind").and_then(Value::as_str).unwrap_or("").into(),
                        s.get("name").and_then(Value::as_str).unwrap_or("").into(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if old.len() != new.sources.len() {
        return Ok(());
    }
    app.store().write(|uow| {
        for ((kind, was), now) in old.iter().zip(&new.sources) {
            if *kind == now.kind && *was != now.name {
                if uow.reads().sources().find(kind, &now.name)?.is_some() {
                    return Err(msg(format!(
                        "a {kind} source named '{}' already has data here; pick another name",
                        now.name
                    )));
                }
                uow.sources().rename(kind, was, &now.name)?;
            }
        }
        Ok(())
    })
}

static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Re-read the file (a hand edit, or after `patch`). Invalid files are refused
/// and reported; the app keeps running on the last good settings.
pub fn reload(app: &App) -> State {
    let parsed = read_file(&app.settings_path()).and_then(|v| parse(&v));
    match parsed {
        Ok(new) => apply(app, new),
        Err(e) => {
            let mut st = app.settings_state();
            st.error = Some(e.to_string());
            st.clone()
        }
    }
}

fn resolved(app: &App, mut s: Settings) -> Settings {
    crate::sync::connectors::obsidian::resolve_config(&mut s.sources, &app.vault);
    crate::sync::connectors::protoncal::resolve_config(&mut s);
    s
}

/// Swap in `new` and apply whatever changed. No change, no new message.
fn apply(app: &App, new: Settings) -> State {
    let new = resolved(app, new);
    let old = app.settings();
    if *old == new {
        let mut st = app.settings_state();
        st.error = None;
        return st.clone();
    }
    app.set_settings(new.clone());
    keep_last_good(&app.settings_path(), &new);
    let mut notes: Vec<String> = vec![];
    let mut restart = false;

    if old.ui_scale != new.ui_scale {
        if let Some(h) = app.hooks.get() {
            (h.set_zoom)(new.ui_scale.clamp(0.5, 3.0));
        }
    }
    if old.obsidian_vault != new.obsidian_vault || old.fake_now != new.fake_now {
        restart = true;
    }
    if old.categories != new.categories {
        match app
            .store()
            .write(|uow| crate::queries::categories::load(uow, &new.categories))
        {
            Ok(true) => {
                let _ = crate::queries::categories::save(app, &app.settings_path());
            }
            Ok(false) => {}
            Err(e) => notes.push(format!("categories not applied: {e}")),
        }
    }
    if new.demo_data && !old.demo_data {
        if let Err(e) = crate::seed::seed_if_empty(app) {
            notes.push(format!("sample data: {e}"));
        }
    }
    let restarted = restart_changed_sources(app, &old.sources, &new.sources);

    let message = if restart && app.hooks.get().is_some() {
        "Saved · restarting".to_string()
    } else if restart {
        "Saved · restart Alethiometer to apply".to_string()
    } else if !restarted.is_empty() {
        // Glean is search-only and Scion re-reads its config: nothing to reconnect.
        let search_only = new
            .sources
            .iter()
            .filter(|s| s.kind == "glean" || s.kind == "scion")
            .map(|s| &s.name)
            .collect::<Vec<_>>();
        let verb = if restarted.iter().all(|n| search_only.contains(&n)) {
            "updated"
        } else {
            "reconnected"
        };
        format!("Saved · {} {verb}", join_names(&restarted))
    } else {
        "Saved · live".to_string()
    };
    let st = {
        let mut st = app.settings_state();
        st.rev += 1;
        st.error = None;
        st.message = if notes.is_empty() {
            message
        } else {
            format!("{message} · {}", notes.join(" · "))
        };
        st.clone()
    };
    if restart {
        if let Some(h) = app.hooks.get() {
            let r = h.restart.clone();
            std::thread::spawn(move || {
                // Let the response reach the UI first.
                std::thread::sleep(std::time::Duration::from_millis(600));
                r()
            });
        }
    }
    st
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Register sources and restart the ones whose settings changed. Returns their names.
fn restart_changed_sources(app: &App, old: &[SourceConfig], new: &[SourceConfig]) -> Vec<String> {
    let ids: Vec<(i64, String)> = {
        if let Err(e) = app.store().write(|uow| crate::sync::register(uow, new)) {
            eprintln!("settings: {e}");
        }
        let gone: Vec<&SourceConfig> = old
            .iter()
            .filter(|o| !new.iter().any(|n| n.kind == o.kind && n.name == o.name))
            .collect();
        if let Err(e) = app
            .store()
            .write(|uow| gone.iter().try_for_each(|g| uow.sources().disable(&g.kind, &g.name)))
        {
            eprintln!("settings: {e}");
        }
        let changed = |s: &SourceConfig| old.iter().find(|o| o.kind == s.kind && o.name == s.name) != Some(s);
        let mut ids = vec![];
        for s in old.iter().chain(new.iter()) {
            let now = new.iter().find(|n| n.kind == s.kind && n.name == s.name);
            let differs = match now {
                Some(n) => changed(n),
                None => true,
            };
            if !differs {
                continue;
            }
            if let Ok(Some(id)) = app.store().read(|r| r.sources().find(&s.kind, &s.name)) {
                if !ids.iter().any(|(i, _)| *i == id) {
                    ids.push((id, s.name.clone()));
                }
            }
        }
        ids
    };
    let mut names = vec![];
    for (id, name) in ids {
        let enabled = new.iter().any(|n| n.name == name && n.enabled);
        if let Some(a) = app.arc().filter(|_| crate::sync::running_threads()) {
            crate::sync::start_source(a, id);
        }
        if enabled && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Restart every source that reads secret `name` (after Replace…). Returns their names.
pub fn restart_sources_using(app: &App, secret: &str) -> Vec<String> {
    let s = app.settings();
    let mut names = vec![];
    for src in s
        .sources
        .iter()
        .filter(|x| x.enabled && sources::secrets(&x.kind, &x.config).iter().any(|k| k.name == secret))
    {
        if let Ok(Some(id)) = app.store().read(|r| r.sources().find(&src.kind, &src.name)) {
            if let Some(a) = app.arc().filter(|_| crate::sync::running_threads()) {
                crate::sync::start_source(a, id);
            }
            names.push(src.name.clone());
        }
    }
    names
}

/// Watch `settings.json` for hand edits (and our own writes, which reload
/// to "no change"). Debounced; one background thread for the process.
pub fn watch(app: Arc<App>) {
    use notify::Watcher;
    std::thread::Builder::new()
        .name("settings-watch".into())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let Ok(mut w) = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                let _ = tx.send(ev);
            }) else {
                eprintln!("settings: no file watcher; hand edits apply on restart");
                return;
            };
            if let Err(e) = w.watch(&app.data_dir, notify::RecursiveMode::NonRecursive) {
                eprintln!("settings: file watcher off ({e}); hand edits apply on restart");
                return;
            }
            let ours = |ev: &notify::Event| {
                ev.paths
                    .iter()
                    .any(|p| p.file_name().is_some_and(|n| n == "settings.json"))
            };
            while let Ok(ev) = rx.recv() {
                if !ev
                    .as_ref()
                    .is_ok_and(|e| ours(e) && !matches!(e.kind, notify::EventKind::Access(_)))
                {
                    continue;
                }
                while rx.recv_timeout(std::time::Duration::from_millis(300)).is_ok() {}
                let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
                let st = reload(&app);
                if let Some(e) = st.error {
                    eprintln!("settings.json: {e}");
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::context::tests::app;

    fn arc() -> (Arc<App>, tempfile::TempDir) {
        let (app, dir) = app();
        write_file(&app.settings_path(), &serde_json::to_value(&*app.settings()).unwrap()).unwrap();
        (App::shared(app), dir)
    }

    fn set(path: Value, value: Value) -> Edit {
        Edit::parse(&json!({ "path": path, "value": value })).unwrap()
    }

    #[test]
    fn a_broken_settings_file_opens_on_the_last_good_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // A good start, with a source and the vault left default.
        let mut good = Settings {
            demo_data: false,
            quiet_days: 9,
            ..Default::default()
        };
        good.sources.push(
            serde_json::from_value(serde_json::json!({ "kind": "jira", "tag": "work", "name": "Jira" })).unwrap(),
        );
        std::fs::write(&path, serde_json::to_string_pretty(&good).unwrap()).unwrap();
        drop(App::open(dir.path()).unwrap());
        for broken in [
            "{ \"quiet_days\": ",                                                            // doesn't parse
            "{ \"quiet_days\": \"7\" }",                                                     // wrong type
            "{ \"sources\": [{ \"kind\": \"jira\", \"tag\": \"Work\", \"name\": \"J\" }] }", // bad tag
            "{ \"categories\": [{ \"key\": \"work\", \"label\": \"Work\", \"color\": \"yellow\" }] }", // bad colour
        ] {
            std::fs::write(&path, broken).unwrap();
            let app = App::open(dir.path()).unwrap_or_else(|e| panic!("didn't open with {broken}: {e}"));
            assert_eq!(app.settings().quiet_days, 9, "last good settings with {broken}");
            assert!(
                app.settings_state()
                    .error
                    .as_deref()
                    .is_some_and(|e| e.contains("last good")),
                "{broken}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "the file is left for the user to fix"
            );
        }
    }

    #[test]
    fn a_bad_category_list_is_refused_before_it_applies() {
        let (app, _dir) = arc();
        let bad = set(
            json!(["categories"]),
            json!([{ "key": "work", "label": "Work", "color": "yellow" }]),
        );
        assert!(patch(&app, &[bad]).is_err());
        assert!(app.settings().validate().is_ok());
    }

    #[test]
    fn mute_adds_a_chat_channel_once_to_the_right_source() {
        let (app, _dir) = arc();
        let source = |name: &str, muted: Value| json!({ "kind": "slack", "tag": "work", "name": name, "enabled": false, "config": { "muted": muted } });
        let file =
            json!({ "demo_data": false, "sources": [source("Other", json!([])), source("Slack", json!(["#random"]))] });
        write_file(&app.settings_path(), &file).unwrap();
        let read =
            || -> Value { serde_json::from_str(&std::fs::read_to_string(app.settings_path()).unwrap()).unwrap() };
        mute(&app, "slack", "Slack", "#alerts").unwrap();
        mute(&app, "slack", "Slack", "#alerts").unwrap();
        assert_eq!(read()["sources"][1]["config"]["muted"], json!(["#random", "#alerts"]));
        assert_eq!(read()["sources"][0]["config"]["muted"], json!([]));
        assert!(mute(&app, "slack", "Nope", "#alerts").is_err());
        assert!(mute(&app, "gmail", "Gmail", "x").is_err());
    }

    #[test]
    fn a_change_writes_the_file_and_applies_live() {
        let (app, _dir) = arc();
        let st = patch(&app, &[set(json!(["quiet_days"]), json!(10))]).unwrap();
        assert_eq!(app.settings().quiet_days, 10);
        assert_eq!(st.message, "Saved · live");
        let file: Value = serde_json::from_str(&std::fs::read_to_string(app.settings_path()).unwrap()).unwrap();
        assert_eq!(file["quiet_days"], 10);
        // The watcher's reload of our own write is a no-op: same rev.
        let again = reload(&app);
        assert_eq!(again.rev, st.rev);
    }

    #[test]
    fn a_harness_edit_works_when_the_file_has_no_harness_list() {
        let (app, _dir) = arc();
        write_file(&app.settings_path(), &json!({ "demo_data": false })).unwrap();
        reload(&app);
        let edit = set(
            json!(["agents", "harnesses", 0, "openshell", "env"]),
            json!({ "AWS_REGION": "us-west-2" }),
        );
        patch(&app, &[edit]).unwrap();
        let s = app.settings();
        assert_eq!(s.agents.harnesses.len(), 2, "the defaults are kept");
        assert_eq!(s.agents.harnesses[0].key, "pi");
        assert_eq!(
            s.agents.harnesses[0].openshell.as_ref().unwrap().env["AWS_REGION"],
            "us-west-2"
        );
    }

    #[test]
    fn invalid_edits_are_refused_and_the_file_is_untouched() {
        let (app, _dir) = arc();
        let before = std::fs::read_to_string(app.settings_path()).unwrap();
        assert!(patch(&app, &[set(json!(["day_start_hour"]), json!(20))]).is_err());
        assert!(patch(&app, &[set(json!(["quiet_days"]), json!("soon"))]).is_err());
        let e = patch(&app, &[set(json!(["ai", "allowed_providers"]), json!(["openai"]))])
            .unwrap_err()
            .to_string();
        assert!(e.contains("by hand"), "{e}");
        assert_eq!(std::fs::read_to_string(app.settings_path()).unwrap(), before);
    }

    #[test]
    fn a_bad_hand_edit_keeps_the_last_good_settings() {
        let (app, _dir) = arc();
        std::fs::write(app.settings_path(), "{ \"quiet_days\": ").unwrap();
        let st = reload(&app);
        assert!(st.error.as_deref().unwrap_or("").contains("doesn't parse"), "{st:?}");
        assert_eq!(app.settings().quiet_days, 7);
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        v["quiet_days"] = json!(3);
        write_file(&app.settings_path(), &v).unwrap();
        let st = reload(&app);
        assert!(st.error.is_none());
        assert_eq!(app.settings().quiet_days, 3);
    }

    #[test]
    fn prices_keys_with_dots_are_one_segment() {
        let (app, _dir) = arc();
        patch(
            &app,
            &[set(
                json!(["ai", "prices", "us.anthropic.claude-opus-5-5"]),
                json!({ "input": 5.0, "output": 25.0 }),
            )],
        )
        .unwrap();
        assert_eq!(app.settings().ai.prices["us.anthropic.claude-opus-5-5"].output, 25.0);
        patch(
            &app,
            &[
                Edit::parse(&json!({ "op": "remove", "path": ["ai", "prices", "us.anthropic.claude-opus-5-5"] }))
                    .unwrap(),
            ],
        )
        .unwrap();
        assert!(app.settings().ai.prices.is_empty());
        assert_eq!(
            path_label(&[json!("ai"), json!("prices"), json!("us.x")]),
            "ai.prices[\"us.x\"]"
        );
    }

    #[test]
    fn sources_add_rename_and_remove() {
        let (app, _dir) = arc();
        let src = json!({ "kind": "jira", "tag": "work", "name": "Work Jira", "config": { "base_url": "https://x.atlassian.net", "email": "a@b" } });
        let st = patch(
            &app,
            &[Edit::parse(&json!({ "op": "push", "path": ["sources"], "value": src })).unwrap()],
        )
        .unwrap();
        assert_eq!(st.message, "Saved · Work Jira reconnected");
        let id: i64 = app
            .db()
            .query_row("SELECT id FROM sources WHERE name = 'Work Jira'", [], |r| r.get(0))
            .unwrap();
        patch(&app, &[set(json!(["sources", 0, "name"]), json!("DEVOPS Jira"))]).unwrap();
        let renamed: i64 = app
            .db()
            .query_row("SELECT id FROM sources WHERE name = 'DEVOPS Jira'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(renamed, id, "a rename keeps the row");
        let e = patch(
            &app,
            &[set(json!(["sources", 0, "config", "lookback_days"]), json!("x"))],
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("sources[0]"), "{e}");
        patch(
            &app,
            &[Edit::parse(&json!({ "op": "remove", "path": ["sources", 0] })).unwrap()],
        )
        .unwrap();
        assert!(app.settings().sources.is_empty());
        let enabled: bool = app
            .db()
            .query_row("SELECT enabled FROM sources WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert!(!enabled, "a removed source stops but keeps its items");
    }

    #[test]
    fn life_calendars_move_into_the_protoncal_source() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            json!({ "life_calendars": ["https://calendar.proton.me/a"], "sources": [] }).to_string(),
        )
        .unwrap();
        migrate_file(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(v.get("life_calendars").is_none());
        assert_eq!(v["sources"][0]["kind"], "protoncal");
        assert_eq!(v["sources"][0]["config"]["urls"][0], "https://calendar.proton.me/a");
        migrate_file(&p).unwrap();
        let again: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(again, v, "runs once");
    }
}

//! Notes left in the stand-in vault (`<data dir>/vault`) after `obsidian_vault`
//! is set. Every read and write moves to the real vault then, so notes saved
//! before it (Reflect, Topics, People) would disappear from the app. At
//! startup the UI offers the ones the real vault doesn't have, to copy over.
//!
//! The user picks the files: with sample data on, the stand-in also holds the
//! sample People notes and daily reflections, at the same paths real ones use,
//! and those must never reach a real vault. Nothing in the real vault is ever
//! replaced. Files left unticked aren't offered again unless they change.
//!
//! - `list`: `{standin, vault, files: [{path, state, modified, preview}]}`;
//!   `state` is `missing` (can be copied) or `differs` (the real vault has a
//!   different file at that path: compare by hand). Empty when no real vault
//!   is set.
//! - `resolve {copy: [path]}`: copy those `missing` files (keeping their
//!   mtime) and stop offering the rest. Returns `{copied, failed: [{path, error}]}`.

use crate::error::{msg, Result};
use crate::state::App;
use crate::time::Ts;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list" => {
            let Some((standin, vault)) = dirs(app) else {
                return Ok(json!({ "files": [] }));
            };
            let files = pending(&standin, &vault, &dismissed_path(app))?;
            let rows: Vec<Value> = files
                .iter()
                .map(|f| {
                    let modified = Ts::from_unix_ms(f.mtime_ms).map(|t| app.clock.local(t).strftime("%Y-%m-%d %H:%M").to_string());
                    json!({ "path": f.rel, "state": f.state, "modified": modified, "preview": preview(&standin.join(&f.rel)) })
                })
                .collect();
            Ok(json!({ "standin": standin.display().to_string(), "vault": vault.display().to_string(), "files": rows }))
        }
        "resolve" => {
            let copy = args
                .get("copy")
                .and_then(Value::as_array)
                .ok_or_else(|| msg("resolve needs 'copy'"))?;
            let copy: Vec<&str> = copy.iter().filter_map(Value::as_str).collect();
            let (standin, vault) = dirs(app).ok_or_else(|| msg("no Obsidian vault is set"))?;
            Ok(resolve(&standin, &vault, &dismissed_path(app), &copy)?)
        }
        _ => Err(crate::api::unknown(name)),
    }
}

/// The stand-in and the real vault, when a real one is set and isn't the stand-in itself.
fn dirs(app: &App) -> Option<(PathBuf, PathBuf)> {
    if app.settings().obsidian_vault.is_empty() {
        return None;
    }
    let standin = app.data_dir.join("vault").canonicalize().ok()?;
    let vault = app.vault.canonicalize().ok()?;
    (standin != vault).then_some((standin, vault))
}

fn dismissed_path(app: &App) -> PathBuf {
    app.data_dir.join("standin-dismissed.json")
}

struct Pending {
    rel: String,
    state: &'static str,
    len: u64,
    mtime_ms: i64,
}

/// Files in the stand-in the real vault doesn't have the same copy of, less
/// the ones dismissed at their current size and mtime.
fn pending(standin: &Path, vault: &Path, dismissed: &Path) -> Result<Vec<Pending>> {
    let gone = read_dismissed(dismissed);
    let mut found = BTreeMap::new();
    walk(standin, standin, &mut found);
    let mut out = vec![];
    for (rel, (len, mtime_ms)) in found {
        if gone.get(&rel).is_some_and(|d| d.len == len && d.mtime_ms == mtime_ms) {
            continue;
        }
        let state = match std::fs::symlink_metadata(vault.join(&rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
            // Same bytes already there: nothing to do (copied before, or by hand).
            _ if same_bytes(&standin.join(&rel), &vault.join(&rel)) => continue,
            _ => "differs",
        };
        out.push(Pending {
            rel,
            state,
            len,
            mtime_ms,
        });
    }
    Ok(out)
}

/// Every regular file under `dir`, by "/"-separated relative path, with its
/// size and mtime. Hidden entries and symlinks are skipped, as the Obsidian
/// scan skips them.
fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, (u64, i64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk(root, &path, out);
        } else if ft.is_file() {
            let (Ok(rel), Ok(meta)) = (path.strip_prefix(root), entry.metadata()) else {
                continue;
            };
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.insert(rel, (meta.len(), meta.modified().map(ms).unwrap_or(0)));
        }
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

fn ms(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

/// The first line of text, after any front matter, cut to 80 characters.
fn preview(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines().map(str::trim);
    let mut first = lines.next();
    if first == Some("---") {
        lines.by_ref().find(|l| *l == "---");
        first = lines.next();
    }
    let line = first.into_iter().chain(lines).find(|l| !l.is_empty()).unwrap_or("");
    let mut s: String = line.chars().take(80).collect();
    if line.chars().nth(80).is_some() {
        s.push('…');
    }
    s
}

fn resolve(standin: &Path, vault: &Path, dismissed: &Path, copy: &[&str]) -> Result<Value> {
    // Only what `list` offers can be copied: a path from the UI never reaches
    // the file system on its own.
    let files = pending(standin, vault, dismissed)?;
    if let Some(p) = copy
        .iter()
        .find(|p| !files.iter().any(|f| f.rel == **p && f.state == "missing"))
    {
        return Err(msg(format!("{p} isn't a stand-in note the vault is missing")));
    }
    let mut copied = 0;
    let mut failed = vec![];
    let mut gone = read_dismissed(dismissed);
    for f in &files {
        if !copy.contains(&f.rel.as_str()) {
            gone.insert(
                f.rel.clone(),
                Dismissed {
                    len: f.len,
                    mtime_ms: f.mtime_ms,
                },
            );
            continue;
        }
        match copy_one(&standin.join(&f.rel), &vault.join(&f.rel)) {
            Ok(()) => copied += 1,
            Err(e) => failed.push(json!({ "path": f.rel, "error": e.to_string() })),
        }
    }
    crate::fsx::write_atomic(dismissed, &serde_json::to_vec_pretty(&gone)?, None)?;
    Ok(json!({ "copied": copied, "failed": failed }))
}

fn copy_one(from: &Path, to: &Path) -> std::io::Result<()> {
    let bytes = std::fs::read(from)?;
    let modified = std::fs::metadata(from)?.modified().ok();
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::fsx::write_new(to, &bytes, modified)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Dismissed {
    len: u64,
    mtime_ms: i64,
}

fn read_dismissed(path: &Path) -> BTreeMap<String, Dismissed> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::state::App;

    /// An app with a real vault set, and its stand-in vault holding `notes`.
    fn app_with(notes: &[(&str, &str)]) -> (App, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        for (rel, text) in notes {
            let p = data.path().join("vault").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let s = Settings {
            demo_data: false,
            obsidian_vault: vault.path().display().to_string(),
            ..Default::default()
        };
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), s, data.path()).unwrap();
        (app, data, vault)
    }

    fn list(app: &App) -> Vec<(String, String)> {
        let v = call(app, "list", &json!({})).unwrap();
        v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["path"].as_str().unwrap().into(), f["state"].as_str().unwrap().into()))
            .collect()
    }

    fn pair(p: &str, s: &str) -> (String, String) {
        (p.into(), s.into())
    }

    #[test]
    fn offers_stand_in_notes_the_real_vault_lacks() {
        let (app, _d, vault) = app_with(&[
            ("Reflections/Daily/2026-09-28.md", "Calm day.\n"),
            ("Topics/Q4 planning.md", "mine\n"),
            ("People/Jordan.md", "same\n"),
            (".obsidian/app.json", "{}"),
        ]);
        std::fs::create_dir_all(vault.path().join("Topics")).unwrap();
        std::fs::write(vault.path().join("Topics/Q4 planning.md"), "theirs\n").unwrap();
        std::fs::create_dir_all(vault.path().join("People")).unwrap();
        std::fs::write(vault.path().join("People/Jordan.md"), "same\n").unwrap();
        assert_eq!(
            list(&app),
            [
                pair("Reflections/Daily/2026-09-28.md", "missing"),
                pair("Topics/Q4 planning.md", "differs")
            ]
        );
    }

    #[test]
    fn nothing_offered_without_a_real_vault() {
        let data = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(data.path().join("vault")).unwrap();
        std::fs::write(data.path().join("vault/a.md"), "x").unwrap();
        let app = App::from_parts(
            crate::db::open_in_memory().unwrap(),
            Settings {
                demo_data: false,
                ..Default::default()
            },
            data.path(),
        )
        .unwrap();
        assert!(list(&app).is_empty());
    }

    #[test]
    fn copies_the_picked_notes_and_stops_offering_the_rest() {
        let (app, _d, vault) = app_with(&[
            ("Reflections/Daily/2026-09-28.md", "Calm day.\n"),
            ("People/Jordan.md", "sample\n"),
        ]);
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_759_000_000);
        std::fs::File::options()
            .write(true)
            .open(app.data_dir.join("vault/Reflections/Daily/2026-09-28.md"))
            .unwrap()
            .set_modified(then)
            .unwrap();

        let out = call(&app, "resolve", &json!({ "copy": ["Reflections/Daily/2026-09-28.md"] })).unwrap();
        assert_eq!(out["copied"], 1);
        let copied = vault.path().join("Reflections/Daily/2026-09-28.md");
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "Calm day.\n");
        assert_eq!(
            std::fs::metadata(&copied).unwrap().modified().unwrap(),
            then,
            "keeps the note's own mtime"
        );
        assert!(
            !vault.path().join("People/Jordan.md").exists(),
            "an unticked note never reaches the vault"
        );
        assert!(list(&app).is_empty(), "the copy matches and Jordan is dismissed");

        // A dismissed note that changes is offered again.
        std::fs::write(app.data_dir.join("vault/People/Jordan.md"), "edited since\n").unwrap();
        assert_eq!(list(&app), [pair("People/Jordan.md", "missing")]);
    }

    #[test]
    fn never_replaces_a_vault_file() {
        let (app, _d, vault) = app_with(&[("Topics/Q4 planning.md", "mine\n")]);
        std::fs::create_dir_all(vault.path().join("Topics")).unwrap();
        std::fs::write(vault.path().join("Topics/Q4 planning.md"), "theirs\n").unwrap();
        let e = call(&app, "resolve", &json!({ "copy": ["Topics/Q4 planning.md"] }))
            .unwrap_err()
            .to_string();
        assert!(e.contains("isn't a stand-in note"), "{e}");
        assert_eq!(
            std::fs::read_to_string(vault.path().join("Topics/Q4 planning.md")).unwrap(),
            "theirs\n"
        );
    }

    #[test]
    fn a_note_created_in_the_vault_meanwhile_is_kept() {
        let (app, _d, vault) = app_with(&[("Reflections/Daily/2026-09-28.md", "Calm day.\n")]);
        let standin = app.data_dir.join("vault").canonicalize().unwrap();
        let vault_dir = vault.path().canonicalize().unwrap();
        let dismissed = dismissed_path(&app);
        let files = pending(&standin, &vault_dir, &dismissed).unwrap();
        assert_eq!(files.len(), 1);
        // Written by Obsidian between the list and the copy.
        std::fs::create_dir_all(vault.path().join("Reflections/Daily")).unwrap();
        std::fs::write(
            vault.path().join("Reflections/Daily/2026-09-28.md"),
            "Written in Obsidian.\n",
        )
        .unwrap();
        let e = copy_one(&standin.join(&files[0].rel), &vault_dir.join(&files[0].rel)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(vault.path().join("Reflections/Daily/2026-09-28.md")).unwrap(),
            "Written in Obsidian.\n"
        );
    }

    #[test]
    fn refuses_paths_it_did_not_offer() {
        let (app, _d, _v) = app_with(&[("a.md", "x")]);
        for p in ["../settings.json", "/etc/passwd", "b.md"] {
            assert!(call(&app, "resolve", &json!({ "copy": [p] })).is_err(), "{p}");
        }
    }

    #[test]
    fn preview_skips_front_matter_and_blank_lines() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\ntags: [x]\n---\n\n# Calm day\n").unwrap();
        assert_eq!(preview(&p), "# Calm day");
    }
}

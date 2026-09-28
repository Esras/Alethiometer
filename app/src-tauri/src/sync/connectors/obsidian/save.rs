//! Saving a whole note the user edited in Alethiometer (a topic's note, a
//! reflection). Shared by Topics and Reflect.
//!
//! With an obsidian source configured for the vault, a save is an outbox row
//! `obsidian.replace {path, base, text}`: the connector writes through the
//! running Obsidian app (or the file, with `write_mode = "file"`) and refuses
//! if the note no longer holds `base`. The UI follows the row with `status`.
//!
//! Without one (the sample data: its Obsidian source is a placeholder with no
//! vault, and there is no Obsidian to talk to) the file under `App.vault` is
//! written directly, guarded by the same base check, so the sample app works.
//!
//! Typing while an earlier save is still queued (or failed: Obsidian closed)
//! doesn't pile up rows: the newer text replaces the queued row's text, and
//! the row keeps its base. That only happens when the new save continues from
//! that row (its `base` is the row's text); otherwise it's a fresh row.

use super::rest::{CONFLICT, NOT_OPEN};
use crate::error::{msg, Result};
use crate::state::App;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::{Component, Path};

pub enum Saved {
    /// Written straight to the file (no obsidian source).
    Written,
    /// The file no longer holds `base`; nothing written. Carries what's there now.
    Conflict(Option<String>),
    /// Queued as this outbox row.
    Queued(i64),
}

/// The enabled obsidian source that reads the app's vault, if any.
pub fn writer(conn: &Connection, vault: &Path) -> Result<Option<i64>> {
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, config_json FROM sources WHERE kind = 'obsidian' AND enabled = 1 ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let want = canon(vault);
    Ok(rows.into_iter().find_map(|(id, cfg)| {
        let cfg: Value = serde_json::from_str(&cfg).ok()?;
        let v = cfg.get("vault").and_then(Value::as_str).filter(|s| !s.is_empty())?;
        (canon(Path::new(v)) == want).then_some(id)
    }))
}

/// Vault-relative, `.md`, no `..`: paths here come from our own tables, but
/// they're joined onto the vault for direct writes.
fn check_rel(rel: &str) -> Result<()> {
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) || !rel.ends_with(".md") {
        return Err(msg(format!("not a note path in the vault: {rel}")));
    }
    Ok(())
}

pub fn disk_text(app: &App, rel: &str) -> Option<String> {
    std::fs::read_to_string(app.vault.join(rel)).ok()
}

/// Save `text` over the note at `rel` (vault-relative), provided it still
/// holds `base` (None: it doesn't exist yet).
pub fn save_note(app: &App, rel: &str, base: Option<&str>, text: &str) -> Result<Saved> {
    check_rel(rel)?;
    let conn = app.db();
    let Some(source) = writer(&conn, &app.vault)? else {
        drop(conn);
        return write_direct(app, rel, base, text);
    };
    // Continue a queued (or failed) row for this note when this save follows from it.
    let queued: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, payload_json FROM outbox
              WHERE source_id = ?1 AND op = 'obsidian.replace' AND state IN ('pending', 'failed')
                AND json_extract(payload_json, '$.path') = ?2
              ORDER BY id DESC LIMIT 1",
            params![source, rel],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, payload)) = queued {
        let mut p: Value = serde_json::from_str(&payload)?;
        if p.get("text").and_then(Value::as_str) == base {
            p["text"] = json!(text);
            let n = conn.execute(
                "UPDATE outbox SET payload_json = ?2 WHERE id = ?1 AND state IN ('pending', 'failed')",
                params![id, p.to_string()],
            )?;
            if n == 1 {
                crate::outbox::wake();
                return Ok(Saved::Queued(id));
            }
            // It was picked up meanwhile: queue behind it.
        }
    }
    let id = crate::outbox::enqueue(&conn, source, "obsidian.replace", &json!({ "path": rel, "base": base, "text": text }), None)?;
    crate::outbox::wake();
    Ok(Saved::Queued(id))
}

fn write_direct(app: &App, rel: &str, base: Option<&str>, text: &str) -> Result<Saved> {
    let abs = app.vault.join(rel);
    let disk = std::fs::read_to_string(&abs).ok();
    if disk.as_deref() == Some(text) {
        return Ok(Saved::Written);
    }
    if disk.as_deref() != base {
        return Ok(Saved::Conflict(disk));
    }
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir)?;
    }
    super::write_checked(&abs, disk.as_deref(), text).map_err(|e| msg(e.to_string()))?;
    Ok(Saved::Written)
}

/// Where a queued save stands, for the editor's header:
/// `saving` | `saved` | `conflict` (+ `disk`, the note as it is now) |
/// `offline` (Obsidian isn't open) | `failed` (+ `error`).
pub fn status(app: &App, outbox_id: i64) -> Result<Value> {
    let (state, error, payload): (String, Option<String>, String) = app.db().query_row(
        "SELECT state, error, payload_json FROM outbox WHERE id = ?1 AND op = 'obsidian.replace'",
        [outbox_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let error = error.unwrap_or_default();
    Ok(match state.as_str() {
        "confirmed" => json!({ "state": "saved" }),
        "failed" if error.starts_with(CONFLICT) => {
            let p: Value = serde_json::from_str(&payload)?;
            let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
            json!({ "state": "conflict", "disk": disk_text(app, path) })
        }
        "failed" if error == NOT_OPEN => json!({ "state": "offline", "error": error }),
        "failed" => json!({ "state": "failed", "error": error }),
        _ => json!({ "state": "saving" }),
    })
}

/// "Keep mine" after a conflict: the user saw the note changed and chose their
/// text. The row's base becomes the note as it is now on disk (which is what
/// Obsidian saved) and it's sent again, still guarded: if Obsidian has moved on
/// since, it's a conflict again.
pub fn keep_mine(app: &App, outbox_id: i64) -> Result<Value> {
    let conn = app.db();
    let (state, error, payload): (String, Option<String>, String) = conn.query_row(
        "SELECT state, error, payload_json FROM outbox WHERE id = ?1 AND op = 'obsidian.replace'",
        [outbox_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if state != "failed" || !error.unwrap_or_default().starts_with(CONFLICT) {
        return Err(msg("that save isn't in conflict"));
    }
    let mut p: Value = serde_json::from_str(&payload)?;
    let path = p.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    p["base"] = json!(disk_text(app, &path));
    conn.execute(
        "UPDATE outbox SET payload_json = ?2, state = 'pending', error = NULL WHERE id = ?1 AND state = 'failed'",
        params![outbox_id, p.to_string()],
    )?;
    drop(conn);
    crate::outbox::wake();
    Ok(json!({ "state": "saving" }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::settings::{Settings, SourceConfig};
    use crate::sync::http::FakeHttp;

    /// Sample data plus a real obsidian source on the sample vault.
    pub(crate) fn app_with_source(write_mode: &str) -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        // Configured after seeding (sources in settings would skip the sample data).
        let mut cfgs = vec![SourceConfig {
            kind: "obsidian".into(),
            tag: "work".into(),
            name: "Work vault".into(),
            enabled: true,
            poll_secs: 60,
            config: json!({ "write_mode": write_mode }),
        }];
        super::super::resolve_config(&mut cfgs, &app.vault);
        crate::sync::register_sources(&app.db(), &cfgs).unwrap();
        app.platform.secret_set(super::super::rest::API_KEY, "k3y").unwrap();
        (app, dir)
    }

    fn note_json(content: &str) -> String {
        json!({ "path": "p.md", "content": content, "stat": { "ctime": 1, "mtime": 2, "size": 3 },
                "tags": [], "frontmatter": {}, "links": [], "backlinks": [], "unresolvedLinks": [] })
        .to_string()
    }

    #[test]
    fn sample_data_writes_directly() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        assert!(writer(&app.db(), &app.vault).unwrap().is_none(), "the sample Obsidian row has no vault");
        let rel = "Topics/Q4 headcount.md";
        let before = disk_text(&app, rel).unwrap();
        assert!(matches!(save_note(&app, rel, Some("stale"), "x").unwrap(), Saved::Conflict(Some(ref d)) if *d == before));
        assert!(matches!(save_note(&app, rel, Some(&before), "mine\n").unwrap(), Saved::Written));
        assert_eq!(disk_text(&app, rel).unwrap(), "mine\n");
        assert!(save_note(&app, "../x.md", None, "x").is_err());
    }

    #[test]
    fn queued_saves_follow_the_outbox() {
        let (app, _d) = app_with_source("obsidian");
        let rel = "Topics/Q4 headcount.md";
        let base = disk_text(&app, rel).unwrap();
        let Saved::Queued(id) = save_note(&app, rel, Some(&base), "v1\n").unwrap() else { panic!() };
        assert_eq!(status(&app, id).unwrap()["state"], "saving");
        // More typing before it's sent: same row, newest text, original base.
        let Saved::Queued(id2) = save_note(&app, rel, Some("v1\n"), "v2\n").unwrap() else { panic!() };
        assert_eq!(id2, id);

        // Obsidian is closed: the row fails with the "isn't open" message (the
        // mapping from a refused connection is tested in rest.rs).
        let http = FakeHttp::default();
        app.db()
            .execute("UPDATE outbox SET state = 'failed', error = ?2 WHERE id = ?1", params![id, NOT_OPEN])
            .unwrap();
        assert_eq!(status(&app, id).unwrap()["state"], "offline");
        // Still typing while it's closed: folded into the failed row.
        let Saved::Queued(id3) = save_note(&app, rel, Some("v2\n"), "v3\n").unwrap() else { panic!() };
        assert_eq!(id3, id);

        // ⌘R, Obsidian open, note unchanged: it lands through the API.
        app.db().execute("UPDATE outbox SET state = 'pending' WHERE id = ?1", [id]).unwrap();
        http.on("GET", "/vault/Topics/Q4%20headcount.md", 200, note_json(&base));
        http.on("PUT", "/vault/Topics/Q4%20headcount.md", 204, "");
        assert!(crate::outbox::process_one(&app, &http).unwrap());
        assert_eq!(status(&app, id).unwrap()["state"], "saved");
        let put: Vec<_> = http.requests().into_iter().filter(|r| r.0 == "PUT").collect();
        assert_eq!(put[0].2.as_deref(), Some("v3\n"));

        // The next save starts a new row from the saved text.
        let Saved::Queued(id4) = save_note(&app, rel, Some("v3\n"), "v4\n").unwrap() else { panic!() };
        assert_ne!(id4, id);
    }

    #[test]
    fn conflicts_then_keep_mine() {
        let (app, _d) = app_with_source("obsidian");
        let rel = "Topics/Q4 headcount.md";
        let base = disk_text(&app, rel).unwrap();
        let Saved::Queued(id) = save_note(&app, rel, Some(&base), "mine\n").unwrap() else { panic!() };
        // Someone edited the note in Obsidian (and it saved to disk).
        std::fs::write(app.vault.join(rel), "theirs\n").unwrap();
        let http = FakeHttp::default();
        http.on("GET", "/vault/", 200, note_json("theirs\n"));
        http.on("PUT", "/vault/", 204, "");
        crate::outbox::process_one(&app, &http).unwrap();
        let s = status(&app, id).unwrap();
        assert_eq!((s["state"].as_str(), s["disk"].as_str()), (Some("conflict"), Some("theirs\n")));
        assert!(http.requests().iter().all(|r| r.0 != "PUT"));

        keep_mine(&app, id).unwrap();
        crate::outbox::process_one(&app, &http).unwrap();
        assert_eq!(status(&app, id).unwrap()["state"], "saved");
        assert!(keep_mine(&app, id).is_err());
    }

    #[test]
    fn file_mode_goes_through_the_outbox_too() {
        let (app, _d) = app_with_source("file");
        let rel = "Topics/Alerting P2.md";
        let base = disk_text(&app, rel).unwrap();
        let Saved::Queued(id) = save_note(&app, rel, Some(&base), "edited\n").unwrap() else { panic!() };
        crate::outbox::process_one(&app, &FakeHttp::default()).unwrap();
        assert_eq!(status(&app, id).unwrap()["state"], "saved");
        assert_eq!(disk_text(&app, rel).unwrap(), "edited\n");
    }
}

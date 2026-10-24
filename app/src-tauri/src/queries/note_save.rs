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

use crate::domain::notes::{CONFLICT, NOT_OPEN};
use crate::error::{msg, Result};
use crate::state::App;
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

/// The enabled obsidian source that reads the app's vault, if any. Paths
/// are compared after the snapshot closes (it touches the filesystem).
pub fn writer(app: &App) -> Result<Option<i64>> {
    let sources = app.store().read(|r| r.sources().of_kind("obsidian"))?;
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let want = canon(&app.vault);
    Ok(sources.into_iter().filter(|s| s.enabled).find_map(|s| {
        let cfg: Value = serde_json::from_str(&s.config_json).ok()?;
        let v = cfg.get("vault").and_then(Value::as_str).filter(|v| !v.is_empty())?;
        (canon(Path::new(v)) == want).then_some(s.id)
    }))
}

/// Vault-relative, `.md`, no `..`: paths here come from our own tables, but
/// they're joined onto the vault for direct writes.
fn check_rel(rel: &str) -> Result<()> {
    let p = Path::new(rel);
    if rel.is_empty()
        || p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
        || !rel.ends_with(".md")
    {
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
    let Some(source) = writer(app)? else {
        return write_direct(app, rel, base, text);
    };
    let id = app.store().write(|uow| {
        // Continue a queued (or failed) row for this note when this save follows from it.
        if let Some(q) = uow.reads().outbox().open_for_path(source, "obsidian.replace", rel)? {
            let mut p: Value = serde_json::from_str(&q.payload_json)?;
            if p.get("text").and_then(Value::as_str) == base {
                p["text"] = json!(text);
                if uow.outbox().update_open(q.id, &p.to_string())? {
                    return Ok(q.id);
                }
                // It was picked up meanwhile: queue behind it.
            }
        }
        crate::outbox::request(
            uow,
            source,
            "obsidian.replace",
            &json!({ "path": rel, "base": base, "text": text }),
            None,
        )
    })?;
    crate::outbox::wake();
    Ok(Saved::Queued(id))
}

/// Merge two People notes in one go (`person_merge`): `keep` = (path, the
/// text it holds, the merged text), `gone` = (path, the text it holds), which
/// goes to the trash. `people` = (kept person, merged-away person): their
/// hand-made topic links move once the merge lands. With an obsidian source
/// it's one outbox row, `obsidian.merge_notes`; without one, the files are
/// written here, the kept note first, both checked against their text.
pub fn merge_notes(app: &App, keep: (&str, &str, &str), gone: (&str, &str), people: (i64, i64)) -> Result<Saved> {
    check_rel(keep.0)?;
    check_rel(gone.0)?;
    let Some(source) = writer(app)? else {
        let disk = disk_text(app, gone.0);
        if disk.as_deref() != Some(gone.1) {
            return Ok(Saved::Conflict(disk));
        }
        match write_direct(app, keep.0, Some(keep.1), keep.2)? {
            Saved::Written => {}
            other => return Ok(other),
        }
        crate::fsx::move_to_trash(&app.vault, &app.vault.join(gone.0), gone.1).map_err(|e| msg(e.to_string()))?;
        app.store()
            .write(|uow| uow.people().move_topic_links(people.1, people.0))?;
        crate::live::Live::vault_changed(app);
        return Ok(Saved::Written);
    };
    let payload = json!({
        "path": keep.0, "base": keep.1, "text": keep.2,
        "trash": { "path": gone.0, "base": gone.1 },
        "people": { "keep": people.0, "gone": people.1 },
    });
    let id = app
        .store()
        .write(|uow| crate::outbox::request(uow, source, "obsidian.merge_notes", &payload, None))?;
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
    crate::fsx::write_checked(&abs, disk.as_deref(), text).map_err(|e| msg(e.to_string()))?;
    crate::live::Live::vault_changed(app);
    Ok(Saved::Written)
}

/// Where a queued save stands, for the editor's header:
/// `saving` | `saved` | `conflict` (+ `disk`, the note as it is now) |
/// `offline` (Obsidian isn't open) | `failed` (+ `error`).
pub fn status(app: &App, outbox_id: i64) -> Result<Value> {
    let row = queued_save(app, outbox_id)?;
    let error = row.error.unwrap_or_default();
    Ok(match row.state.as_str() {
        "confirmed" => json!({ "state": "saved" }),
        "failed" if error.starts_with(CONFLICT) => {
            let p: Value = serde_json::from_str(&row.payload_json)?;
            let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
            json!({ "state": "conflict", "disk": disk_text(app, path) })
        }
        "failed" if error == NOT_OPEN => json!({ "state": "offline", "error": error }),
        "failed" => json!({ "state": "failed", "error": error }),
        _ => json!({ "state": "saving" }),
    })
}

fn queued_save(app: &App, outbox_id: i64) -> Result<crate::domain::outbox::OutboxEntry> {
    app.store()
        .read(|r| r.outbox().get(outbox_id))?
        .filter(|e| e.op == "obsidian.replace")
        .ok_or_else(|| msg("no such save"))
}

/// "Keep mine" after a conflict: the user saw the note changed and chose their
/// text. The row's base becomes the note as it is now on disk (which is what
/// Obsidian saved) and it's sent again, still guarded: if Obsidian has moved on
/// since, it's a conflict again.
pub fn keep_mine(app: &App, outbox_id: i64) -> Result<Value> {
    let row = queued_save(app, outbox_id)?;
    if row.state != "failed" || !row.error.unwrap_or_default().starts_with(CONFLICT) {
        return Err(msg("that save isn't in conflict"));
    }
    let mut p: Value = serde_json::from_str(&row.payload_json)?;
    let path = p.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    p["base"] = json!(disk_text(app, &path));
    if !app
        .store()
        .write(|uow| uow.outbox().resend(outbox_id, &p.to_string()))?
    {
        return Err(msg("that save isn't in conflict"));
    }
    crate::outbox::wake();
    Ok(json!({ "state": "saving" }))
}

#[cfg(test)]
pub(crate) mod tests;

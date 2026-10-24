//! Merging two People notes (design canvas frames `MergeFromPage` and
//! `MergeModal`; decisions in `app/docs/decisions.md` › "People tab design").
//!
//! - One note is kept, the other goes to the trash. The kept note stays as
//!   written; the other's frontmatter is folded in:
//!   - `aliases` and the account fields (`slack`, `teams`, `email`, `jira`,
//!     `github`) are unioned (`add_to_frontmatter`, so the kept note's layout
//!     stays);
//!   - any other field the kept note lacks is copied over;
//!   - a field both have with different values is a conflict: the kept note's
//!     value stays unless the user takes the other side (`take`).
//! - The other note's body is appended under `## Merged from <name>`.
//! - Written in one go (`note_save::merge_notes`): the outbox op
//!   `obsidian.merge_notes` through Obsidian, or the files directly without an
//!   Obsidian source. Either refuses if a note changed since the preview.
//!   Once it lands, topics linked to the merged-away person by hand link to
//!   the kept one.

use super::note_save::{disk_text, merge_notes, Saved};
use super::people::{add_to_frontmatter, fence, field, parse_frontmatter, rescan, set_field, KINDS};
use crate::error::{msg, Result};
use crate::state::App;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Keys whose values are unioned rather than picked.
fn is_list(key: &str) -> bool {
    key == "aliases" || KINDS.contains(&key)
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Conflict {
    pub key: String,
    pub keep: Vec<String>,
    pub gone: Vec<String>,
}

/// Fields both notes set to different values.
pub fn conflicts(keep: &str, gone: &str) -> Vec<Conflict> {
    let kf = parse_frontmatter(keep);
    parse_frontmatter(gone)
        .into_iter()
        .filter(|(k, v)| !is_list(k) && !v.is_empty())
        .filter_map(|(key, gone)| {
            let keep = field(&kf, &key);
            (!keep.is_empty() && keep != gone.as_slice()).then(|| Conflict {
                keep: keep.to_vec(),
                key,
                gone,
            })
        })
        .collect()
}

/// The note's text after its frontmatter.
fn body(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    match fence(&lines) {
        Some((_, end)) => lines[end + 1..].join("\n"),
        None => text.to_string(),
    }
}

/// `keep` with `gone` folded in; `take` lists the conflicting keys where
/// `gone`'s value wins.
pub fn merged(keep: &str, gone: &str, gone_name: &str, take: &HashSet<String>) -> String {
    let nl = if keep.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = keep.to_string();
    for (key, values) in parse_frontmatter(gone) {
        if is_list(&key) {
            for v in &values {
                if let Some(t) = add_to_frontmatter(&out, &key, v) {
                    out = t;
                }
            }
            continue;
        }
        if values.is_empty() {
            continue;
        }
        let kf = parse_frontmatter(&out);
        let kept = field(&kf, &key);
        if kept.is_empty() || (kept != values.as_slice() && take.contains(&key)) {
            out = set_field(&out, &key, &values);
        }
    }
    let rest = body(gone);
    let rest = rest.trim();
    if !rest.is_empty() {
        let rest = rest.replace("\r\n", "\n").replace('\n', nl);
        out = format!("{}{nl}{nl}## Merged from {gone_name}{nl}{rest}{nl}", out.trim_end());
    }
    out
}

#[derive(Serialize, Debug)]
struct Side {
    id: i64,
    name: String,
    path: String,
    text: String,
}

/// Both notes, read from the vault after a short read of their paths.
fn sides(app: &App, args: &Value) -> Result<(Side, Side, HashSet<String>)> {
    let keep = super::arg_i64(args, "keep")?;
    let gone = super::arg_i64(args, "gone")?;
    if keep == gone {
        return Err(msg("a person can't be merged into themselves"));
    }
    let (k, g) = app
        .store()
        .read(|r| Ok((r.people().note_of(keep)?, r.people().note_of(gone)?)))?;
    let side = |id: i64, note: Option<(String, String)>| -> Result<Side> {
        let (path, name) = note.ok_or_else(|| msg("that person has no note"))?;
        let text = disk_text(app, &path).ok_or_else(|| msg(format!("{path} isn't in the vault any more")))?;
        Ok(Side { id, name, path, text })
    };
    let take = args
        .get("take")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    Ok((side(keep, k)?, side(gone, g)?, take))
}

/// What the merge would write: both notes as they are, the conflicts, and the
/// kept note after the merge with `take` applied.
pub fn preview(app: &App, args: &Value) -> Result<Value> {
    let (keep, gone, take) = sides(app, args)?;
    Ok(json!({
        "conflicts": conflicts(&keep.text, &gone.text),
        "merged": merged(&keep.text, &gone.text, &gone.name, &take),
        "keep": keep,
        "gone": gone,
    }))
}

/// Merge `gone` into `keep` as `preview` showed it: `keep_text` and
/// `gone_text` are the notes the preview read. A note that has changed since
/// is a conflict, and nothing is written.
pub fn merge(app: &App, args: &Value) -> Result<Value> {
    let (mut keep, mut gone, take) = sides(app, args)?;
    keep.text = super::arg_str(args, "keep_text")?;
    gone.text = super::arg_str(args, "gone_text")?;
    let text = merged(&keep.text, &gone.text, &gone.name, &take);
    let saved = merge_notes(
        app,
        (&keep.path, &keep.text, &text),
        (&gone.path, &gone.text),
        (keep.id, gone.id),
    )?;
    match saved {
        Saved::Conflict(_) => Err(msg(format!(
            "{} or {} changed while you were merging; nothing was written. Try again.",
            keep.path, gone.path
        ))),
        Saved::Written => {
            rescan(app)?;
            Ok(json!({ "state": "written", "path": keep.path, "trashed": gone.path }))
        }
        Saved::Queued(outbox_id) => {
            Ok(json!({ "state": "queued", "path": keep.path, "trashed": gone.path, "outbox_id": outbox_id }))
        }
    }
}

#[cfg(test)]
mod tests;

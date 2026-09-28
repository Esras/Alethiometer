//! Obsidian work vault: plain Markdown files on disk.
//!
//! Poll: walk the vault, re-read files whose mtime changed since the cursor,
//! and emit one `note` item per file plus one `task` item per checkbox line.
//! The cursor is a map of every file's mtime rather than a single "max mtime",
//! because renames and files synced in from elsewhere keep old mtimes, and
//! deletions only show up as a missing entry.
//!
//! Notes are the user's own writing, so they're never Incoming signal; new
//! tasks are (unless they sit in a topic's note, see `after_apply`).
//!
//! Writeback (outbox ops) goes through the running Obsidian app by default
//! (`rest`, the Local REST API plugin), so the user's editor buffers and sync
//! plugin see ordinary Obsidian edits; `config.write_mode = "file"` writes the
//! files directly instead. Either way every op reads the note first and never
//! merges: `obsidian.replace` needs the note to still hold the text the edit
//! started from, `append`/`toggle_task` need their target to be unambiguous.
//! Reads (the poll) always scan the files.

pub mod rest;
pub mod save;

use crate::error::Result;
use crate::seed::fmt;
use crate::sync::{Batch, Connector, Ctx, ItemUpsert, Source};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

pub struct Obsidian;

/// Always skipped: Obsidian's own config, its trash, Alethiometer's reflections (they
/// can hold the Life lens, which must never become work items) and templates.
const DEFAULT_IGNORE: &[&str] = &[".obsidian/", ".trash/", "Reflections/", "Templates/", "templates/"];

/// Note bodies are stored for search; very long files are cut.
const BODY_CAP: usize = 64 * 1024;

impl Connector for Obsidian {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        let vault = vault_root(&ctx.source.config)?;
        let ignore = Ignore::from_config(&ctx.source.config);
        let prev = Cursor::parse(ctx.source.cursor.as_deref());
        let mut files = BTreeMap::new();
        scan(&vault, &vault, &ignore, &mut files)?;

        let mut items = Vec::new();
        for (rel, mtime) in &files {
            if prev.files.get(rel) == Some(mtime) {
                continue;
            }
            let abs = vault.join(rel);
            let Ok(bytes) = std::fs::read(&abs) else { continue }; // vanished mid-scan: next poll
            let text = String::from_utf8_lossy(&bytes);
            let at = local_from_ms(*mtime);
            items.extend(note_items(&vault, rel, &at, &text));
        }
        let cursor = serde_json::to_string(&Cursor { files })?;
        Ok(Batch { items, cursor: Some(cursor), ..Default::default() })
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        let vault = vault_root(&ctx.source.config)?;
        match write_mode(&ctx.source.config)? {
            WriteMode::File => write_op(&Store::File, &vault, op, payload),
            WriteMode::Obsidian => {
                // Tests hand in a FakeHttp through ctx; the app needs the pinned client.
                let own = if cfg!(test) { None } else { Some(rest::transport(&ctx.source.config, &vault)?) };
                let url = match &own {
                    Some((url, _)) => url.clone(),
                    None => ctx.source.config.get("rest_url").and_then(Value::as_str).unwrap_or(rest::DEFAULT_URL).to_string(),
                };
                let http = own.as_ref().map_or(ctx.http, |(_, h)| h.as_ref());
                let key = ctx.require_secret(rest::API_KEY)?;
                write_op(&Store::Rest(rest::Rest::new(http, &url, &key)), &vault, op, payload)
            }
        }
    }
}

/// How writes reach the vault. `obsidian` (default): through the running app's
/// Local REST API plugin, so editors and the user's sync plugin stay in step.
/// `file`: straight to disk (temp file + rename), for vaults without the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Obsidian,
    File,
}

pub fn write_mode(config: &Value) -> anyhow::Result<WriteMode> {
    match config.get("write_mode").and_then(Value::as_str) {
        None | Some("obsidian") => Ok(WriteMode::Obsidian),
        Some("file") => Ok(WriteMode::File),
        Some(m) => anyhow::bail!("obsidian config.write_mode must be \"obsidian\" or \"file\", not \"{m}\""),
    }
}

/// Where a write op reads the current text and writes the new one.
enum Store<'a> {
    File,
    Rest(rest::Rest<'a>),
}

impl Store<'_> {
    /// Current text; None when the note doesn't exist.
    fn read(&self, abs: &Path, rel: &str) -> anyhow::Result<Option<String>> {
        match self {
            Store::File => abs.exists().then(|| read_text(abs)).transpose(),
            Store::Rest(api) => Ok(api.read(rel)?.map(|n| n.content)),
        }
    }

    /// Write `after`. `before` is what `read` just returned: the file store
    /// re-checks it right before its rename; the REST store has no conditional
    /// write, and its read happened a moment ago.
    fn write(&self, abs: &Path, rel: &str, before: Option<&str>, after: &str) -> anyhow::Result<()> {
        match self {
            Store::File => write_checked(abs, before, after).map(|_| ()),
            Store::Rest(api) => api.write(rel, after),
        }
    }

    fn receipt(&self, abs: &Path, rel: &str, changed: bool, line: Option<usize>) -> Value {
        let mut r = json!({ "path": rel, "changed": changed });
        if let Some(line) = line {
            r["line"] = json!(line);
        }
        match self {
            Store::File => {
                if let Ok(m) = std::fs::metadata(abs).and_then(|m| m.modified()) {
                    r["mtime"] = json!(fmt(local_naive(m)));
                    r["mtime_ms"] = json!(ms(m));
                }
                r["via"] = json!("file");
            }
            Store::Rest(_) => r["via"] = json!("obsidian"),
        }
        r
    }
}

fn write_op(store: &Store, vault: &Path, op: &str, payload: &Value) -> anyhow::Result<Value> {
    let path = payload.get("path").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("{op}: payload needs path"))?;
    let str_arg = |k: &str| payload.get(k).and_then(Value::as_str);
    match op {
        // The whole note, as the user edited it in Alethiometer. `base` is the
        // text they started from (null: the note didn't exist). Anything else in
        // the note now is a conflict: nothing is written, nothing is merged.
        "obsidian.replace" => {
            let text = str_arg("text").ok_or_else(|| anyhow::anyhow!("obsidian.replace: payload needs text"))?;
            let base = match payload.get("base") {
                Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.as_str()),
                _ => anyhow::bail!("obsidian.replace: payload needs base (the text the edit started from, or null)"),
            };
            let (abs, rel) = target(vault, path, true)?;
            if let (Store::File, Some(dir)) = (store, abs.parent()) {
                // `target` checked that the nearest existing folder is inside the vault.
                std::fs::create_dir_all(dir)?;
            }
            let now = store.read(&abs, &rel)?;
            // Already there (a retry after the write landed): done.
            if now.as_deref() == Some(text) {
                return Ok(store.receipt(&abs, &rel, false, None));
            }
            if now.as_deref() != base {
                anyhow::bail!("{} the note changed in Obsidian since you started editing; nothing was written", rest::CONFLICT);
            }
            store.write(&abs, &rel, now.as_deref(), text)?;
            Ok(store.receipt(&abs, &rel, true, None))
        }
        "obsidian.append" => {
            let text = str_arg("text").ok_or_else(|| anyhow::anyhow!("obsidian.append: payload needs text"))?;
            let create = payload.get("create").and_then(Value::as_bool).unwrap_or(false);
            let (abs, rel) = target(vault, path, create)?;
            let before = store.read(&abs, &rel)?;
            if before.is_none() && !create {
                anyhow::bail!("note not found: {path}");
            }
            let (after, line) = append_text(before.as_deref().unwrap_or(""), text, str_arg("under_heading"))?;
            if let Some(after) = &after {
                store.write(&abs, &rel, before.as_deref(), after)?;
            }
            Ok(store.receipt(&abs, &rel, after.is_some(), Some(line)))
        }
        "obsidian.toggle_task" => {
            let task = str_arg("task_text").ok_or_else(|| anyhow::anyhow!("obsidian.toggle_task: payload needs task_text"))?;
            let done = payload.get("done").and_then(Value::as_bool).ok_or_else(|| anyhow::anyhow!("obsidian.toggle_task: payload needs done"))?;
            let (abs, rel) = target(vault, path, false)?;
            let before = store.read(&abs, &rel)?.ok_or_else(|| anyhow::anyhow!("note not found: {path}"))?;
            let (after, line) = toggle_text(&before, task, done).map_err(|e| anyhow::anyhow!("{rel}: {e}"))?;
            if let Some(after) = &after {
                store.write(&abs, &rel, Some(&before), after)?;
            }
            Ok(store.receipt(&abs, &rel, after.is_some(), Some(line)))
        }
        _ => anyhow::bail!("Obsidian doesn't support '{op}'"),
    }
}

/// Fill `config.vault` for obsidian sources that don't name one:
/// `settings.obsidian_vault`, else the stand-in `<data dir>/vault` (both already
/// resolved into `App.vault`). No source is added automatically: reading the
/// whole vault into Alethiometer is a choice the user should make in `sources`.
pub fn resolve_config(sources: &mut [crate::settings::SourceConfig], vault: &Path) {
    for s in sources.iter_mut().filter(|s| s.kind == "obsidian") {
        if !s.config.is_object() {
            s.config = json!({});
        }
        if s.config.get("vault").and_then(Value::as_str).is_none_or(str::is_empty) {
            s.config["vault"] = json!(vault.to_string_lossy());
        }
    }
}

fn vault_root(config: &Value) -> anyhow::Result<PathBuf> {
    let v = config.get("vault").and_then(Value::as_str).filter(|s| !s.is_empty());
    let v = v.ok_or_else(|| anyhow::anyhow!("obsidian source needs config.vault (or settings.obsidian_vault)"))?;
    let p = PathBuf::from(v);
    if !p.is_dir() {
        anyhow::bail!("vault folder not found: {v}");
    }
    Ok(p.canonicalize()?)
}

// ---------------------------------------------------------------- scanning

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Cursor {
    /// Vault-relative path ("/"-separated) → mtime in ms.
    files: BTreeMap<String, i64>,
}

impl Cursor {
    fn parse(s: Option<&str>) -> Cursor {
        s.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
    }
}

/// gitignore-lite: `dir/` matches a folder, a pattern with a `/` is anchored
/// at the vault root, one without matches the file name. `*` stays within a
/// path segment, `**` crosses them, `?` is one character.
struct Ignore(Vec<String>);

impl Ignore {
    fn from_config(config: &Value) -> Ignore {
        let extra = config.get("ignore").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        Ignore(DEFAULT_IGNORE.iter().copied().chain(extra).map(str::to_owned).collect())
    }

    fn matches(&self, rel: &str, is_dir: bool) -> bool {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        self.0.iter().any(|p| {
            let (p, dir_only) = match p.strip_suffix('/') {
                Some(p) => (p, true),
                None => (p.as_str(), false),
            };
            if dir_only && !is_dir {
                return false;
            }
            match p.strip_prefix('/') {
                Some(anchored) => glob(anchored, rel),
                None if p.contains('/') => glob(p, rel),
                None => glob(p, name),
            }
        })
    }
}

fn glob(p: &str, s: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', rest @ ..] => {
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            [b'*', rest @ ..] => (0..=s.len()).take_while(|&i| i == 0 || s[i - 1] != b'/').any(|i| go(rest, &s[i..])),
            [b'?', rest @ ..] => !s.is_empty() && s[0] != b'/' && go(rest, &s[1..]),
            [c, rest @ ..] => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(p.as_bytes(), s.as_bytes())
}

/// Every `.md` file under `dir`. Hidden entries (.git, .obsidian…) and
/// symlinks are skipped: symlinks could lead out of the vault or loop.
fn scan(root: &Path, dir: &Path, ignore: &Ignore, out: &mut BTreeMap<String, i64>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        let Some(rel) = rel_str(root, &path) else { continue };
        if ft.is_dir() {
            if !ignore.matches(&rel, true) {
                // An unreadable subfolder shouldn't fail the whole vault.
                let _ = scan(root, &path, ignore, out);
            }
        } else if ft.is_file() && name.ends_with(".md") && !ignore.matches(&rel, false) {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                out.insert(rel, ms(m));
            }
        }
    }
    Ok(())
}

/// Would the scanner read this vault-relative path?
fn wanted(ignore: &Ignore, rel: &str) -> bool {
    let segs: Vec<&str> = rel.split('/').collect();
    rel.ends_with(".md")
        && !segs.iter().any(|s| s.starts_with('.'))
        && !ignore.matches(rel, false)
        && !(1..segs.len()).any(|n| ignore.matches(&segs[..n].join("/"), true))
}

fn rel_str(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Some(parts.join("/"))
}

fn ms(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

fn local_naive(t: SystemTime) -> chrono::NaiveDateTime {
    chrono::DateTime::<chrono::Local>::from(t).naive_local()
}

fn local_from_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| fmt(t.with_timezone(&chrono::Local).naive_local()))
        .unwrap_or_default()
}

// ---------------------------------------------------------------- parsing

/// `obsidian://open?path=<absolute path>`; every reserved character encoded,
/// `/` included, as the Obsidian URI docs require.
pub fn obsidian_url(abs: &Path) -> String {
    let mut out = String::from("obsidian://open?path=");
    for b in abs.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s*(?:[-*+]|\d+[.)])\s+\[)(.)(\]\s+)(.*?)\s*$").unwrap())
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap())
}

fn due_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Tasks plugin `📅 2026-10-01`; Dataview `due:: 2026-10-01`, optionally in [] or ().
    RE.get_or_init(|| Regex::new(r"📅\s*(\d{4}-\d{2}-\d{2})|[\[(]?\bdue::\s*(\d{4}-\d{2}-\d{2})[\])]?").unwrap())
}

fn done_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"✅\s*\d{4}-\d{2}-\d{2}").unwrap())
}

/// The task's words without due/done markers. Ids hash this, so adding a due
/// date doesn't turn a task into a new one.
fn clean_task(text: &str) -> String {
    let t = due_re().replace_all(text, "");
    let t = done_date_re().replace_all(&t, "");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn due_of(text: &str) -> Option<String> {
    let c = due_re().captures(text)?;
    let d = c.get(1).or_else(|| c.get(2))?.as_str();
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
    // Date-only due; end of the working day, like the rest of the app.
    Some(format!("{d}T{}", crate::queries::DAY_ONLY_DUE))
}

/// `x`, `X` and `-` (cancelled) close a task; other markers (` `, `/`, `>`…) leave it open.
fn is_done(mark: &str) -> bool {
    matches!(mark, "x" | "X" | "-")
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Lines outside fenced code blocks, with their index.
fn prose_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut fence = false;
    text.lines().enumerate().filter(move |(_, l)| {
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            return false;
        }
        !fence
    })
}

fn summary_of(text: &str) -> Option<String> {
    let mut lines = text.lines().peekable();
    // Skip YAML front matter.
    if lines.peek().map(|l| l.trim()) == Some("---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
        }
    }
    lines
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```"))
        .map(|l| l.trim_start_matches(['-', '*', '+', '>', ' ']).chars().take(200).collect())
}

fn cap(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut i = n;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

fn note_items(vault: &Path, rel: &str, at: &str, text: &str) -> Vec<ItemUpsert> {
    let stem = rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".md").to_string();
    let url = obsidian_url(&vault.join(rel));
    let mut out = vec![ItemUpsert {
        external_id: format!("note:{rel}"),
        kind: "note".into(),
        title: stem.clone(),
        body: cap(text, BODY_CAP).to_string(),
        summary: summary_of(text),
        url: Some(url.clone()),
        occurred_at: at.to_string(),
        status: "open".into(),
        raw_json: json!({ "path": rel }),
        ..Default::default()
    }];
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for (i, line) in prose_lines(text) {
        let Some(c) = task_re().captures(line) else { continue };
        let raw = c[4].to_string();
        let title = clean_task(&raw);
        if title.is_empty() {
            continue;
        }
        let h = hash(&title);
        let n = seen.entry(h).or_insert(0);
        *n += 1;
        // The same task text twice in one note: number the repeats.
        let suffix = if *n > 1 { format!("-{n}") } else { String::new() };
        out.push(ItemUpsert {
            external_id: format!("task:{rel}#{h:016x}{suffix}"),
            kind: "task".into(),
            title,
            body: line.trim().to_string(),
            summary: Some(stem.clone()),
            url: Some(url.clone()),
            occurred_at: at.to_string(),
            due_at: due_of(&raw),
            status: if is_done(&c[2]) { "done" } else { "open" }.into(),
            owner: Some("you".into()),
            raw_json: json!({ "path": rel, "text": raw, "line": i + 1 }),
            ..Default::default()
        });
    }
    out
}

// ---------------------------------------------------------------- after apply

/// Database follow-up after `sync::apply` for an obsidian batch. Returns how
/// many items it took out of Incoming.
///
/// - The first sync is a baseline: everything imported is marked seen, so a
///   vault's worth of old tasks doesn't flood Unsorted.
/// - Notes are never signal (they're the user's own writing): marked seen.
/// - A topic's own note and the tasks in it are linked to that topic as
///   `confirmed`, because the user put them there; and marked seen. An
///   existing link (including `wrong`) is left alone.
/// - Tasks that disappeared from a re-read note (checked off, edited, or
///   deleted) and everything from a deleted or renamed file are set `done`.
///   Rows are never deleted.
pub fn after_apply(conn: &Connection, source: &Source, batch: &Batch) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let baseline = source.cursor.is_none();
    let topics: HashMap<String, i64> = tx
        .prepare("SELECT obsidian_path, id FROM topics WHERE archived_at IS NULL AND obsidian_path IS NOT NULL")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .filter_map(|r| r.ok())
        .map(|(p, id)| (p.trim_start_matches("./").trim_start_matches('/').to_string(), id))
        .collect();

    let mut quieted = 0;
    let mut present: HashMap<&str, HashSet<&str>> = HashMap::new();
    for it in &batch.items {
        let Some(path) = it.raw_json.get("path").and_then(Value::as_str) else { continue };
        let row: Option<(i64, bool)> = tx
            .query_row(
                "SELECT id, seen FROM items WHERE source_id = ?1 AND external_id = ?2",
                params![source.id, it.external_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, seen)) = row else { continue };
        let topic = topics.get(path);
        if let Some(t) = topic {
            tx.execute(
                "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
                 VALUES (?1, ?2, 'confirmed', 1.0, 'in the topic''s note') ON CONFLICT DO NOTHING",
                params![id, t],
            )?;
        }
        if !seen && (baseline || it.kind == "note" || it.status == "done" || topic.is_some()) {
            tx.execute("UPDATE items SET seen = 1 WHERE id = ?1", [id])?;
            quieted += 1;
        }
        let tasks = present.entry(path).or_default();
        if it.kind == "task" {
            tasks.insert(it.external_id.as_str());
        }
    }

    // Tasks gone from notes we just re-read.
    for (path, ids) in &present {
        close_tasks(&tx, source.id, path, ids)?;
    }
    // Files gone since the last scan.
    if let Some(new) = &batch.cursor {
        let (old, new) = (Cursor::parse(source.cursor.as_deref()), Cursor::parse(Some(new)));
        for path in old.files.keys().filter(|p| !new.files.contains_key(*p)) {
            tx.execute(
                "UPDATE items SET status = 'done' WHERE source_id = ?1 AND external_id = ?2",
                params![source.id, format!("note:{path}")],
            )?;
            close_tasks(&tx, source.id, path, &HashSet::new())?;
        }
    }
    tx.commit()?;
    Ok(quieted)
}

fn close_tasks(conn: &Connection, source_id: i64, path: &str, keep: &HashSet<&str>) -> Result<()> {
    let prefix = format!("task:{path}#");
    let open: Vec<(i64, String)> = conn
        .prepare(
            "SELECT id, external_id FROM items WHERE source_id = ?1 AND kind = 'task' AND status = 'open'
               AND substr(external_id, 1, length(?2)) = ?2",
        )?
        .query_map(params![source_id, prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, ext) in open {
        if !keep.contains(ext.as_str()) {
            conn.execute("UPDATE items SET status = 'done' WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- live

/// The obsidian source's sync loop: poll on start, then again whenever a
/// Markdown file changes (debounced) or at least every `poll` (capped at 60 s;
/// the scan is only a local stat walk). Without a watcher it's a plain timer.
/// One thread per source, so two scans never race over the cursor.
pub fn run(app: std::sync::Arc<crate::state::App>, source_id: i64, name: String, poll: std::time::Duration) {
    use notify::Watcher;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    let poll = poll.min(Duration::from_secs(60));
    std::thread::Builder::new()
        .name(format!("sync-{name}"))
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
            let config = crate::sync::load_source(&app.db(), source_id).map(|s| s.config).unwrap_or(Value::Null);
            let ignore = Ignore::from_config(&config);
            let root = vault_root(&config).ok();
            let _watcher = root.as_ref().and_then(|root| {
                let mut w = notify::recommended_watcher(move |ev| {
                    let _ = tx.send(ev);
                })
                .ok()?;
                match w.watch(root, notify::RecursiveMode::Recursive) {
                    Ok(()) => Some(w),
                    Err(e) => {
                        eprintln!("sync {name}: file watcher off ({e}); polling every {}s", poll.as_secs());
                        None
                    }
                }
            });
            let relevant = |ev: &notify::Event| {
                !matches!(ev.kind, notify::EventKind::Access(_))
                    && ev.paths.iter().any(|p| root.as_ref().and_then(|r| rel_str(r, p)).is_some_and(|rel| wanted(&ignore, &rel)))
            };
            loop {
                if let Err(e) = crate::sync::sync_source(&app, source_id) {
                    eprintln!("sync {name}: {e}");
                }
                let deadline = Instant::now() + poll;
                loop {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(Ok(ev)) if relevant(&ev) => {
                            // Editors save in bursts; wait for a quiet second.
                            while rx.recv_timeout(Duration::from_millis(1000)).is_ok() {}
                            break;
                        }
                        Ok(_) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                            break;
                        }
                    }
                }
            }
        })
        .ok();
}

// ---------------------------------------------------------------- writeback

/// Resolve a payload path (vault-relative, or absolute inside the vault) to a
/// file in the vault. Refuses `..`, paths outside the vault, symlinks that
/// lead out, and anything but `.md`.
fn target(vault: &Path, path: &str, allow_missing: bool) -> anyhow::Result<(PathBuf, String)> {
    let p = Path::new(path);
    let rel: &Path = if p.is_absolute() {
        p.strip_prefix(vault).map_err(|_| anyhow::anyhow!("refusing a path outside the vault: {path}"))?
    } else {
        p
    };
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        anyhow::bail!("refusing a path outside the vault: {path}");
    }
    if rel.extension().and_then(|e| e.to_str()) != Some("md") {
        anyhow::bail!("only Markdown notes can be edited: {path}");
    }
    let abs = vault.join(rel);
    let check = if abs.exists() {
        abs.canonicalize()?
    } else if allow_missing {
        // The nearest folder that exists decides (a symlink there could lead out).
        let dir = abs.ancestors().skip(1).find(|a| a.exists()).ok_or_else(|| anyhow::anyhow!("bad path: {path}"))?;
        dir.canonicalize()?
    } else {
        anyhow::bail!("note not found: {path}");
    };
    if !check.starts_with(vault) {
        anyhow::bail!("refusing a path outside the vault: {path}");
    }
    let rel = rel_str(vault, &abs).unwrap_or_else(|| path.to_string());
    Ok((abs, rel))
}

fn read_text(abs: &Path) -> anyhow::Result<String> {
    String::from_utf8(std::fs::read(abs)?).map_err(|_| anyhow::anyhow!("note isn't UTF-8; not editing it"))
}

/// Write `after` over `abs` via temp file + rename, but only if the file still
/// holds `before` (None = must not exist yet). Returns the new mtime.
fn write_checked(abs: &Path, before: Option<&str>, after: &str) -> anyhow::Result<SystemTime> {
    use std::io::Write;
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Hidden name: the scanner and the watcher filter skip it.
    let tmp = abs.with_file_name(format!(".{name}.alethiometer-tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(after.as_bytes())?;
        f.sync_all()?;
    }
    let unchanged = match before {
        Some(b) => std::fs::read(abs).ok().as_deref() == Some(b.as_bytes()),
        None => !abs.exists(),
    };
    if !unchanged {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("the note changed while writing; retry");
    }
    if let Ok(meta) = std::fs::metadata(abs) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, abs) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(std::fs::metadata(abs)?.modified()?)
}

/// Line ending the file already uses.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Byte offset just past line `i` (including its line break).
fn line_end(text: &str, i: usize) -> usize {
    text.split_inclusive('\n').take(i + 1).map(str::len).sum()
}

/// Append one line, at the end or at the end of a heading's section (before
/// trailing blank lines). A missing heading is added at the end of the file.
/// `under_heading` may carry its level ("## Asks"), or just the text ("Asks").
/// Returns the new text and the 1-based line number written.
///
/// Idempotent, so a retried outbox row can't add the line twice: when the
/// exact line already sits where it would go (the last non-blank line of the
/// section, or of the file), nothing changes and the text is None. The price:
/// appending the same line twice in a row on purpose is a no-op.
fn append_text(orig: &str, text: &str, under_heading: Option<&str>) -> anyhow::Result<(Option<String>, usize)> {
    let text = text.trim_end_matches(['\r', '\n']);
    if text.contains('\n') || text.contains('\r') {
        anyhow::bail!("obsidian.append takes a single line");
    }
    let nl = newline(orig);
    let mut out = orig.to_string();
    let ensure_break = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
    };
    let Some(h) = under_heading.map(str::trim).filter(|h| !h.is_empty()) else {
        let lines: Vec<&str> = orig.lines().collect();
        if let Some(i) = (0..lines.len()).rev().find(|&i| !lines[i].trim().is_empty()).filter(|&i| lines[i] == text) {
            return Ok((None, i + 1));
        }
        ensure_break(&mut out);
        out.push_str(text);
        out.push_str(nl);
        let n = out.lines().count();
        return Ok((Some(out), n));
    };
    let (want_level, want_text) = match heading_re().captures(h) {
        Some(c) => (Some(c[1].len()), c[2].to_string()),
        None => (None, h.to_string()),
    };
    let headings: Vec<(usize, usize, String)> = prose_lines(orig)
        .filter_map(|(i, l)| heading_re().captures(l).map(|c| (i, c[1].len(), c[2].to_string())))
        .collect();
    let hits: Vec<&(usize, usize, String)> =
        headings.iter().filter(|(_, lvl, t)| *t == want_text && want_level.is_none_or(|w| w == *lvl)).collect();
    match hits.as_slice() {
        [] => {
            ensure_break(&mut out);
            if !out.is_empty() && !out.ends_with(&format!("{nl}{nl}")) {
                out.push_str(nl);
            }
            let hashes = "#".repeat(want_level.unwrap_or(2));
            out.push_str(&format!("{hashes} {want_text}{nl}{text}{nl}"));
            let n = out.lines().count();
            Ok((Some(out), n))
        }
        [(at, lvl, _)] => {
            let lines: Vec<&str> = orig.lines().collect();
            let end = headings.iter().find(|(i, l, _)| i > at && l <= lvl).map_or(lines.len(), |(i, _, _)| *i);
            let last = (at + 1..end).rev().find(|&i| !lines[i].trim().is_empty()).unwrap_or(*at);
            if last != *at && lines[last] == text {
                return Ok((None, last + 1));
            }
            let pos = line_end(orig, last);
            let mut insert = String::new();
            if !orig[..pos].ends_with('\n') {
                insert.push_str(nl);
            }
            insert.push_str(text);
            insert.push_str(nl);
            out.insert_str(pos, &insert);
            Ok((Some(out), last + 2))
        }
        _ => anyhow::bail!("heading '{h}' appears more than once; not guessing which"),
    }
}

/// Flip exactly one checkbox. The task is found by its words (due/done
/// markers ignored); none or several matches is an error. Returns None when
/// it's already in the wanted state.
fn toggle_text(orig: &str, task_text: &str, done: bool) -> anyhow::Result<(Option<String>, usize)> {
    let want = clean_task(task_text);
    let hits: Vec<(usize, &str)> = prose_lines(orig)
        .filter(|(_, l)| task_re().captures(l).is_some_and(|c| clean_task(&c[4]) == want))
        .collect();
    let (i, line) = match hits.as_slice() {
        [] => anyhow::bail!("task not found (the note changed?): {want}"),
        [one] => *one,
        _ => anyhow::bail!("task appears {} times; not guessing which: {want}", hits.len()),
    };
    let c = task_re().captures(line).expect("matched above");
    if is_done(&c[2]) == done {
        return Ok((None, i + 1));
    }
    let line_start = line_end(orig, i) - orig.split_inclusive('\n').nth(i).map_or(0, str::len);
    let mark = line_start + c.get(2).unwrap().start();
    let mut out = String::with_capacity(orig.len());
    out.push_str(&orig[..mark]);
    out.push_str(if done { "x" } else { " " });
    out.push_str(&orig[mark + c[2].len()..]);
    Ok((Some(out), i + 1))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SourceConfig;
    use crate::sync::http::FakeHttp;

    struct Vault {
        dir: tempfile::TempDir,
    }

    impl Vault {
        fn new() -> Vault {
            Vault { dir: tempfile::tempdir().unwrap() }
        }
        fn root(&self) -> PathBuf {
            self.dir.path().canonicalize().unwrap()
        }
        fn write(&self, rel: &str, text: &str) {
            let p = self.dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.dir.path().join(rel)).unwrap()
        }
        fn config(&self) -> Value {
            json!({ "vault": self.dir.path().to_string_lossy(), "write_mode": "file" })
        }
    }

    fn src(v: &Vault, cursor: Option<String>) -> Source {
        Source { id: 1, kind: "obsidian".into(), tag: "work".into(), name: "Vault".into(), config: v.config(), cursor }
    }

    fn poll(s: &Source) -> Batch {
        let http = FakeHttp::default();
        let secret = |_: &str| None;
        let ctx = Ctx { source: s, http: &http, secret: &secret, now: chrono::Local::now().naive_local() };
        Obsidian.poll(&ctx).unwrap()
    }

    fn send(v: &Vault, op: &str, payload: Value) -> anyhow::Result<Value> {
        let s = src(v, None);
        let http = FakeHttp::default();
        let secret = |_: &str| None;
        let ctx = Ctx { source: &s, http: &http, secret: &secret, now: chrono::Local::now().naive_local() };
        Obsidian.send(&ctx, op, &payload)
    }

    /// An op through the REST API (the default write mode), answered by `http`.
    fn send_rest(v: &Vault, http: &FakeHttp, op: &str, payload: Value) -> anyhow::Result<Value> {
        let mut s = src(v, None);
        s.config["write_mode"] = json!("obsidian");
        let secret = |k: &str| (k == rest::API_KEY).then(|| "k3y".to_string());
        let ctx = Ctx { source: &s, http, secret: &secret, now: chrono::Local::now().naive_local() };
        Obsidian.send(&ctx, op, &payload)
    }

    fn note_json(content: &str) -> String {
        json!({ "path": "x.md", "content": content, "frontmatter": {}, "tags": [], "links": [], "backlinks": [], "unresolvedLinks": [],
                "stat": { "ctime": 1788001200000u64, "mtime": 1790592723000u64, "size": content.len() } })
        .to_string()
    }

    fn puts(http: &FakeHttp) -> Vec<(String, Option<String>)> {
        http.requests().into_iter().filter(|r| r.0 == "PUT").map(|r| (r.1, r.2)).collect()
    }

    #[test]
    fn rest_replace_writes_through_obsidian() {
        let v = Vault::new();
        v.write("Topics/T.md", "old on disk\n");
        let http = FakeHttp::default();
        http.on("GET", "/vault/Topics/T.md", 200, note_json("## A\n- one\n"));
        http.on("PUT", "/vault/Topics/T.md", 204, "");
        let r = send_rest(&v, &http, "obsidian.replace", json!({ "path": "Topics/T.md", "base": "## A\n- one\n", "text": "## B\n- moved\n\n## A\n- one\n" })).unwrap();
        assert_eq!((r["changed"].as_bool(), r["via"].as_str()), (Some(true), Some("obsidian")));
        assert_eq!(puts(&http), vec![("https://127.0.0.1:27124/vault/Topics/T.md".to_string(), Some("## B\n- moved\n\n## A\n- one\n".to_string()))]);
        // The file on disk is Obsidian's business in this mode.
        assert_eq!(v.read("Topics/T.md"), "old on disk\n");
        let h = http.header_log.lock().unwrap();
        assert!(h.iter().all(|h| h.contains(&("Authorization".to_string(), "Bearer k3y".to_string()))));
    }

    #[test]
    fn rest_replace_conflict_writes_nothing() {
        let v = Vault::new();
        v.write("T.md", "x");
        let http = FakeHttp::default();
        http.on("GET", "/vault/T.md", 200, note_json("## A\n- one\n- added in Obsidian\n"));
        let e = send_rest(&v, &http, "obsidian.replace", json!({ "path": "T.md", "base": "## A\n- one\n", "text": "mine" })).unwrap_err().to_string();
        assert!(e.starts_with(rest::CONFLICT), "{e}");
        assert!(puts(&http).is_empty());
        // A retry after the write already landed is a no-op, not a conflict.
        http.on("GET", "/vault/T.md", 200, note_json("mine"));
        let r = send_rest(&v, &http, "obsidian.replace", json!({ "path": "T.md", "base": "## A\n- one\n", "text": "mine" })).unwrap();
        assert_eq!(r["changed"], false);
        assert!(puts(&http).is_empty());
        // base is required, so nothing is written unguarded.
        assert!(send_rest(&v, &http, "obsidian.replace", json!({ "path": "T.md", "text": "y" })).is_err());
    }

    #[test]
    fn rest_replace_creates_a_missing_note_only_from_a_null_base() {
        let v = Vault::new();
        let http = FakeHttp::default();
        http.on("GET", "/vault/Reflections/Daily/2026-09-28.md", 404, r#"{"errorCode":40400,"message":"Not Found"}"#);
        http.on("PUT", "/vault/Reflections/Daily/2026-09-28.md", 204, "");
        let p = "Reflections/Daily/2026-09-28.md";
        assert!(send_rest(&v, &http, "obsidian.replace", json!({ "path": p, "base": "old", "text": "new" })).unwrap_err().to_string().starts_with("conflict"));
        send_rest(&v, &http, "obsidian.replace", json!({ "path": p, "base": null, "text": "Calm.\n" })).unwrap();
        assert_eq!(puts(&http).len(), 1);
        // Paths are still checked against the vault.
        assert!(send_rest(&v, &http, "obsidian.replace", json!({ "path": "../out.md", "base": null, "text": "x" })).is_err());
    }

    #[test]
    fn rest_append_and_toggle_read_modify_put() {
        let v = Vault::new();
        v.write("Topics/T.md", "");
        let http = FakeHttp::default();
        http.on("GET", "/vault/Topics/T.md", 200, note_json("# T\n\n## Asks\n- [ ] one\n\n## Links\n- x\n"));
        http.on("PUT", "/vault/Topics/T.md", 204, "");
        let r = send_rest(&v, &http, "obsidian.append", json!({ "path": "Topics/T.md", "text": "- [ ] two", "under_heading": "Asks" })).unwrap();
        assert_eq!((r["changed"].as_bool(), r["line"].as_i64()), (Some(true), Some(5)));
        assert_eq!(puts(&http)[0].1.as_deref(), Some("# T\n\n## Asks\n- [ ] one\n- [ ] two\n\n## Links\n- x\n"));
        // Idempotent through the API too: the line is already there, no PUT.
        http.on("GET", "/vault/Topics/T.md", 200, note_json("# T\n\n## Asks\n- [ ] one\n- [ ] two\n"));
        let r = send_rest(&v, &http, "obsidian.append", json!({ "path": "Topics/T.md", "text": "- [ ] two", "under_heading": "Asks" })).unwrap();
        assert_eq!(r["changed"], false);
        assert_eq!(puts(&http).len(), 1);

        let r = send_rest(&v, &http, "obsidian.toggle_task", json!({ "path": "Topics/T.md", "task_text": "one", "done": true })).unwrap();
        assert_eq!(r["line"], 4);
        assert_eq!(puts(&http)[1].1.as_deref(), Some("# T\n\n## Asks\n- [x] one\n- [ ] two\n"));
        // A missing note is refused for toggles and for appends without create.
        http.on("GET", "/vault/Topics/T.md", 404, "{}");
        assert!(send_rest(&v, &http, "obsidian.toggle_task", json!({ "path": "Topics/T.md", "task_text": "one", "done": true })).is_err());
        assert!(send_rest(&v, &http, "obsidian.append", json!({ "path": "Topics/T.md", "text": "x" })).is_err());
    }

    #[test]
    fn rest_needs_the_api_key_and_explains_a_bad_one() {
        let v = Vault::new();
        v.write("T.md", "a\n");
        let http = FakeHttp::default();
        http.on("GET", "/vault/T.md", 401, r#"{"errorCode":40101,"message":"Authorization required."}"#);
        let e = send_rest(&v, &http, "obsidian.replace", json!({ "path": "T.md", "base": "a\n", "text": "b" })).unwrap_err().to_string();
        assert!(e.contains("refused the API key"), "{e}");
        let mut s = src(&v, None);
        s.config["write_mode"] = json!("obsidian");
        let none = |_: &str| None;
        let ctx = Ctx { source: &s, http: &http, secret: &none, now: chrono::Local::now().naive_local() };
        let e = Obsidian.send(&ctx, "obsidian.replace", &json!({ "path": "T.md", "base": "a\n", "text": "b" })).unwrap_err().to_string();
        assert!(e.contains("obsidian.api_key"), "{e}");
        assert!(write_mode(&json!({ "write_mode": "disk" })).is_err());
    }

    #[test]
    fn file_mode_replace_checks_the_base() {
        let v = Vault::new();
        v.write("T.md", "theirs\n");
        let e = send(&v, "obsidian.replace", json!({ "path": "T.md", "base": "mine before\n", "text": "mine\n" })).unwrap_err().to_string();
        assert!(e.starts_with("conflict"), "{e}");
        assert_eq!(v.read("T.md"), "theirs\n");
        let r = send(&v, "obsidian.replace", json!({ "path": "T.md", "base": "theirs\n", "text": "mine\n" })).unwrap();
        assert_eq!((r["changed"].as_bool(), r["via"].as_str()), (Some(true), Some("file")));
        assert_eq!(v.read("T.md"), "mine\n");
        // New notes in new folders.
        send(&v, "obsidian.replace", json!({ "path": "Reflections/Daily/d.md", "base": null, "text": "x\n" })).unwrap();
        assert_eq!(v.read("Reflections/Daily/d.md"), "x\n");
    }

    /// Move a file's mtime forward so the next scan sees a change even within the same ms.
    fn touch_later(v: &Vault, rel: &str) {
        let f = std::fs::File::options().write(true).open(v.dir.path().join(rel)).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    }

    const TOPIC_NOTE: &str = "---\ntags: [vendor]\n---\n## Where we are\n- Offer: 12% off for 3y\n\n## Asks for Thu\n- [ ] True-down at 12 months 📅 2026-10-01\n- [x] 30d log retention included\n- [ ] Cap on overage rate [due:: 2026-10-03]\n\n```\n- [ ] not a task, in code\n```\n";

    #[test]
    fn scan_emits_notes_and_tasks_and_skips_ignored() {
        let v = Vault::new();
        v.write("Topics/Datadog renewal.md", TOPIC_NOTE);
        v.write("Inbox/Idea #1.md", "# Title\n\nFirst real line\n1. [ ] numbered task\n");
        v.write(".obsidian/workspace.md", "- [ ] nope");
        v.write("Reflections/Life/Day/2026-09-28.md", "- [ ] personal, never read");
        v.write("Templates/Meeting.md", "- [ ] template task");
        v.write("Archive/old.md", "- [ ] archived");
        v.write("notes.txt", "- [ ] not markdown");
        let mut s = src(&v, None);
        s.config["ignore"] = json!(["Archive/"]);
        let b = poll(&s);
        let ids: Vec<&str> = b.items.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(ids.iter().filter(|i| i.starts_with("note:")).count(), 2, "{ids:?}");
        assert!(ids.iter().all(|i| !i.contains("Reflections") && !i.contains("Templates") && !i.contains("Archive")));

        let note = b.items.iter().find(|i| i.external_id == "note:Topics/Datadog renewal.md").unwrap();
        assert_eq!(note.title, "Datadog renewal");
        assert_eq!(note.summary.as_deref(), Some("Offer: 12% off for 3y"));
        assert!(note.body.contains("True-down"));
        let url = note.url.as_deref().unwrap();
        assert!(url.starts_with("obsidian://open?path=%2F"), "{url}");
        assert!(url.ends_with("%2FTopics%2FDatadog%20renewal.md"), "{url}");
        assert!(!note.bump);

        let tasks: Vec<&ItemUpsert> =
            b.items.iter().filter(|i| i.kind == "task" && i.raw_json["path"] == "Topics/Datadog renewal.md").collect();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].title, "True-down at 12 months");
        assert_eq!(tasks[0].due_at.as_deref(), Some("2026-10-01T23:59:59"));
        assert_eq!(tasks[0].owner.as_deref(), Some("you"));
        assert_eq!(tasks[0].summary.as_deref(), Some("Datadog renewal"));
        assert_eq!((tasks[1].status.as_str(), tasks[1].due_at.as_deref()), ("done", None));
        assert_eq!((tasks[2].title.as_str(), tasks[2].due_at.as_deref()), ("Cap on overage rate", Some("2026-10-03T23:59:59")));
        let numbered = b.items.iter().find(|i| i.kind == "task" && i.title == "numbered task").unwrap();
        assert_eq!(numbered.status, "open");
        let idea = b.items.iter().find(|i| i.title == "Idea #1").unwrap();
        assert_eq!(idea.summary.as_deref(), Some("First real line"));
        assert!(idea.url.as_deref().unwrap().ends_with("Idea%20%231.md"));
    }

    #[test]
    fn cursor_rescans_only_changed_files() {
        let v = Vault::new();
        v.write("a.md", "- [ ] one\n");
        v.write("b.md", "text\n");
        let b1 = poll(&src(&v, None));
        assert_eq!(b1.items.len(), 3);
        let b2 = poll(&src(&v, b1.cursor.clone()));
        assert!(b2.items.is_empty());
        v.write("a.md", "- [ ] one\n- [ ] two\n");
        touch_later(&v, "a.md");
        let b3 = poll(&src(&v, b2.cursor.clone()));
        assert_eq!(b3.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["a", "one", "two"]);
        // A rename keeps the mtime but shows up as a new path.
        std::fs::rename(v.dir.path().join("b.md"), v.dir.path().join("c.md")).unwrap();
        let b4 = poll(&src(&v, b3.cursor.clone()));
        assert_eq!(b4.items.iter().map(|i| i.external_id.as_str()).collect::<Vec<_>>(), vec!["note:c.md"]);
    }

    #[test]
    fn task_ids_are_stable_and_ignore_due_edits() {
        let a = note_items(Path::new("/v"), "n.md", "2026-09-28T10:00:00", "- [ ] Ship it\n- [ ] Ship it\n");
        let b = note_items(Path::new("/v"), "n.md", "2026-09-28T10:00:00", "- [ ] Ship it 📅 2026-10-09\n- [ ] Ship it\n");
        assert_eq!(a[1].external_id, b[1].external_id);
        assert_eq!(a[2].external_id, format!("{}-2", a[1].external_id));
        assert_eq!(b[1].due_at.as_deref(), Some("2026-10-09T23:59:59"));
    }

    fn db_with_topic(v: &Vault) -> (Connection, Source) {
        let conn = crate::db::open_in_memory().unwrap();
        let mut cfgs = vec![SourceConfig {
            kind: "obsidian".into(),
            tag: "work".into(),
            name: "Vault".into(),
            enabled: true,
            poll_secs: 60,
            config: json!({}),
        }];
        resolve_config(&mut cfgs, v.dir.path());
        crate::sync::register_sources(&conn, &cfgs).unwrap();
        conn.execute(
            "INSERT INTO topics (name, category, obsidian_path) VALUES ('Datadog renewal', 'work', 'Topics/Datadog renewal.md')",
            [],
        )
        .unwrap();
        let s = crate::sync::load_source(&conn, 1).unwrap();
        (conn, s)
    }

    fn sync(conn: &Connection, id: i64) {
        let s = crate::sync::load_source(conn, id).unwrap();
        let b = poll(&s);
        crate::sync::apply(conn, &s, &b).unwrap();
        after_apply(conn, &s, &b).unwrap();
    }

    type Row = (String, String, bool, Option<String>);

    fn rows(conn: &Connection) -> Vec<Row> {
        conn.prepare(
            "SELECT i.title, i.status, i.seen, (SELECT state FROM topic_links WHERE item_id = i.id)
               FROM items i ORDER BY i.id",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    fn row(r: &[Row], title: &str) -> Row {
        r.iter().find(|x| x.0 == title).unwrap_or_else(|| panic!("{title} not in {r:?}")).clone()
    }

    #[test]
    fn after_apply_baselines_links_and_closes_vanished() {
        let v = Vault::new();
        v.write("Topics/Datadog renewal.md", "## Asks\n- [ ] True-down\n");
        v.write("Daily/2026-09-28.md", "- [ ] Old task\n");
        let (conn, s) = db_with_topic(&v);
        assert!(s.config["vault"].as_str().is_some());

        // First sync is a baseline: nothing is new signal. The topic note and its task are linked.
        sync(&conn, s.id);
        let r = rows(&conn);
        assert!(r.iter().all(|x| x.2), "{r:?}");
        let linked: Vec<&str> = r.iter().filter(|x| x.3.as_deref() == Some("confirmed")).map(|x| x.0.as_str()).collect();
        assert_eq!(linked, vec!["Datadog renewal", "True-down"]);

        // A new task in a daily note is signal; one in the topic note is already filed.
        v.write("Daily/2026-09-28.md", "- [ ] Old task\n- [ ] Call Kim\n");
        touch_later(&v, "Daily/2026-09-28.md");
        v.write("Topics/Datadog renewal.md", "## Asks\n- [x] True-down\n- [ ] Cap overage\n");
        touch_later(&v, "Topics/Datadog renewal.md");
        sync(&conn, s.id);
        let r = rows(&conn);
        assert_eq!(row(&r, "Call Kim"), ("Call Kim".into(), "open".into(), false, None));
        assert_eq!(row(&r, "Cap overage"), ("Cap overage".into(), "open".into(), true, Some("confirmed".into())));
        assert_eq!(row(&r, "True-down").1, "done");
        // Editing a note never resurfaces it.
        assert!(row(&r, "2026-09-28").2);

        // Removing a task line or a whole file closes items; nothing is deleted.
        v.write("Daily/2026-09-28.md", "- [ ] Call Kim\n");
        touch_later(&v, "Daily/2026-09-28.md");
        sync(&conn, s.id);
        assert_eq!(row(&rows(&conn), "Old task").1, "done");
        std::fs::remove_file(v.dir.path().join("Daily/2026-09-28.md")).unwrap();
        sync(&conn, s.id);
        let r = rows(&conn);
        assert_eq!(row(&r, "2026-09-28").1, "done");
        assert_eq!(row(&r, "Call Kim").1, "done");
        let n: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 6);

        // A link the user marked wrong stays wrong.
        conn.execute("UPDATE topic_links SET state = 'wrong'", []).unwrap();
        v.write("Topics/Datadog renewal.md", "## Asks\n- [ ] Cap overage\n- [ ] One more\n");
        touch_later(&v, "Topics/Datadog renewal.md");
        sync(&conn, s.id);
        let r = rows(&conn);
        assert_eq!(row(&r, "Cap overage").3.as_deref(), Some("wrong"));
        assert_eq!(row(&r, "One more").3.as_deref(), Some("confirmed"));
    }

    #[test]
    fn append_under_heading_and_at_end() {
        let v = Vault::new();
        v.write("Topics/T.md", "# T\n\n## Asks\n- [ ] one\n\n## Links\n- x\n");
        let r = send(&v, "obsidian.append", json!({ "path": "Topics/T.md", "text": "- [ ] two", "under_heading": "Asks" })).unwrap();
        assert_eq!(v.read("Topics/T.md"), "# T\n\n## Asks\n- [ ] one\n- [ ] two\n\n## Links\n- x\n");
        assert_eq!(r["line"], 5);
        assert_eq!(r["changed"], true);
        assert_eq!(r["mtime"].as_str().unwrap().len(), 19);
        // A missing heading is created at the end.
        send(&v, "obsidian.append", json!({ "path": "Topics/T.md", "text": "- note", "under_heading": "### Log" })).unwrap();
        assert!(v.read("Topics/T.md").ends_with("- x\n\n### Log\n- note\n"));
        // No heading, no trailing newline, CRLF kept.
        v.write("c.md", "a\r\nb");
        send(&v, "obsidian.append", json!({ "path": "c.md", "text": "c" })).unwrap();
        assert_eq!(v.read("c.md"), "a\r\nb\r\nc\r\n");
        // A section that runs to the end of the file.
        v.write("e.md", "## Asks\n- one");
        send(&v, "obsidian.append", json!({ "path": "e.md", "text": "- two", "under_heading": "Asks" })).unwrap();
        assert_eq!(v.read("e.md"), "## Asks\n- one\n- two\n");
    }

    #[test]
    fn append_is_idempotent_so_a_retry_adds_nothing() {
        let v = Vault::new();
        let orig = "# T\n\n## Asks\n- [ ] one\n\n## Links\n- x\n";
        v.write("T.md", orig);
        let append = |p: Value| send(&v, "obsidian.append", p).unwrap();
        let under = json!({ "path": "T.md", "text": "- [ ] two", "under_heading": "Asks" });
        assert_eq!(append(under.clone())["changed"], true);
        let after = v.read("T.md");
        let r = append(under.clone());
        assert_eq!((r["changed"].as_bool(), r["line"].as_i64()), (Some(false), Some(5)));
        assert_eq!(r["mtime"].as_str().unwrap().len(), 19);
        assert_eq!(v.read("T.md"), after);

        // Only the section's last line counts: the same words elsewhere are appended.
        assert_eq!(append(json!({ "path": "T.md", "text": "- [ ] one", "under_heading": "Links" }))["changed"], true);
        assert_eq!(append(json!({ "path": "T.md", "text": "- [ ] one", "under_heading": "Asks" }))["changed"], true);
        assert_eq!(v.read("T.md"), "# T\n\n## Asks\n- [ ] one\n- [ ] two\n- [ ] one\n\n## Links\n- x\n- [ ] one\n");
        // A heading alone isn't mistaken for the line.
        v.write("h.md", "## Asks\n");
        assert_eq!(append(json!({ "path": "h.md", "text": "## Asks", "under_heading": "Asks" }))["changed"], true);

        // End of file, past trailing blank lines; CRLF files too.
        v.write("e.md", "a\r\nb\r\n\r\n");
        let r = append(json!({ "path": "e.md", "text": "b" }));
        assert_eq!((r["changed"].as_bool(), r["line"].as_i64()), (Some(false), Some(2)));
        assert_eq!(v.read("e.md"), "a\r\nb\r\n\r\n");
        // Exact line only: different whitespace is a different line.
        assert_eq!(append(json!({ "path": "e.md", "text": "b " }))["changed"], true);

        // A missing heading created by the first attempt is found by the retry.
        let log = json!({ "path": "T.md", "text": "- note", "under_heading": "### Log" });
        assert_eq!(append(log.clone())["changed"], true);
        assert_eq!(append(log)["changed"], false);
        // Creating a note, retried.
        let new = json!({ "path": "n.md", "text": "x", "create": true });
        assert_eq!(append(new.clone())["changed"], true);
        assert_eq!(append(new)["changed"], false);
        assert_eq!(v.read("n.md"), "x\n");
    }

    #[test]
    fn append_refuses_ambiguity_and_multiline() {
        let v = Vault::new();
        v.write("d.md", "## Notes\n- a\n## Notes\n- b\n```\n## Notes\n```\n");
        let e = send(&v, "obsidian.append", json!({ "path": "d.md", "text": "x", "under_heading": "Notes" })).unwrap_err();
        assert!(e.to_string().contains("more than once"), "{e}");
        assert!(send(&v, "obsidian.append", json!({ "path": "d.md", "text": "x\ny" })).is_err());
        assert!(send(&v, "obsidian.append", json!({ "path": "missing.md", "text": "x" })).is_err());
        send(&v, "obsidian.append", json!({ "path": "new.md", "text": "x", "create": true })).unwrap();
        assert_eq!(v.read("new.md"), "x\n");
    }

    #[test]
    fn toggle_flips_exactly_one_checkbox() {
        let v = Vault::new();
        v.write("t.md", "- [ ] Alpha 📅 2026-10-01\n  - [ ] Beta\n- [x] Gamma\n");
        let r = send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Beta", "done": true })).unwrap();
        assert_eq!(v.read("t.md"), "- [ ] Alpha 📅 2026-10-01\n  - [x] Beta\n- [x] Gamma\n");
        assert_eq!((r["line"].as_i64(), r["changed"].as_bool()), (Some(2), Some(true)));
        // Found by its words even without the due marker; reopening works.
        send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Alpha", "done": true })).unwrap();
        send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Gamma", "done": false })).unwrap();
        assert_eq!(v.read("t.md"), "- [x] Alpha 📅 2026-10-01\n  - [x] Beta\n- [ ] Gamma\n");
        // Already in that state: no write.
        let r = send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Beta", "done": true })).unwrap();
        assert_eq!(r["changed"], false);
    }

    #[test]
    fn toggle_detects_conflicts() {
        let v = Vault::new();
        v.write("t.md", "- [ ] Same\n- [ ] Same\n- [ ] Other\n");
        let e = send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Same", "done": true })).unwrap_err();
        assert!(e.to_string().contains("2 times"), "{e}");
        let e = send(&v, "obsidian.toggle_task", json!({ "path": "t.md", "task_text": "Renamed", "done": true })).unwrap_err();
        assert!(e.to_string().contains("not found"), "{e}");
        assert_eq!(v.read("t.md"), "- [ ] Same\n- [ ] Same\n- [ ] Other\n");
        // The file changing between read and rename is caught, and the temp file cleaned up.
        let abs = v.root().join("t.md");
        let e = write_checked(&abs, Some("stale"), "new").unwrap_err();
        assert!(e.to_string().contains("changed"), "{e}");
        assert_eq!(v.read("t.md"), "- [ ] Same\n- [ ] Same\n- [ ] Other\n");
        assert!(!v.root().join(".t.md.alethiometer-tmp").exists());
    }

    #[test]
    fn refuses_paths_outside_the_vault() {
        let v = Vault::new();
        v.write("in.md", "x\n");
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("out.md"), "secret\n").unwrap();
        for p in ["../out.md", "a/../../out.md", "/etc/passwd", "in.txt", ""] {
            assert!(send(&v, "obsidian.append", json!({ "path": p, "text": "x", "create": true })).is_err(), "{p}");
        }
        let abs_out = outside.path().join("out.md");
        assert!(send(&v, "obsidian.append", json!({ "path": abs_out.to_string_lossy(), "text": "x" })).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), v.dir.path().join("link")).unwrap();
            let e = send(&v, "obsidian.append", json!({ "path": "link/out.md", "text": "x" })).unwrap_err();
            assert!(e.to_string().contains("outside"), "{e}");
        }
        assert_eq!(std::fs::read_to_string(&abs_out).unwrap(), "secret\n");
        // Absolute paths inside the vault are fine.
        let abs_in = v.root().join("in.md");
        send(&v, "obsidian.append", json!({ "path": abs_in.to_string_lossy(), "text": "y" })).unwrap();
        assert_eq!(v.read("in.md"), "x\ny\n");
    }

    #[test]
    fn watcher_syncs_soon_after_a_save() {
        let v = Vault::new();
        v.write("a.md", "- [ ] first\n");
        let data = tempfile::tempdir().unwrap();
        let settings = crate::settings::Settings {
            demo_data: false,
            obsidian_vault: v.dir.path().to_string_lossy().into_owned(),
            sources: vec![SourceConfig {
                kind: "obsidian".into(),
                tag: "work".into(),
                name: "Vault".into(),
                enabled: true,
                poll_secs: 60,
                config: json!({}),
            }],
            ..Default::default()
        };
        let conn = crate::db::open_in_memory().unwrap();
        let app = std::sync::Arc::new(crate::state::App::from_parts(conn, settings, data.path()).unwrap());
        run(app.clone(), 1, "Vault".into(), std::time::Duration::from_secs(60));
        let count = |t: &str| -> i64 {
            app.db().query_row("SELECT count(*) FROM items WHERE title = ?1", [t], |r| r.get(0)).unwrap()
        };
        let wait = |t: &str| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while count(t) == 0 && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            count(t)
        };
        assert_eq!(wait("first"), 1);
        v.write("b.md", "- [ ] second\n");
        // Well before the 60 s poll: only the watcher can have picked it up.
        assert_eq!(wait("second"), 1);
    }

    #[test]
    fn ignore_globs() {
        let ig = Ignore(vec!["Archive/".into(), "*.excalidraw.md".into(), "/Daily/**/old-*.md".into()]);
        assert!(ig.matches("Archive", true));
        assert!(!ig.matches("Archive", false));
        assert!(ig.matches("x/y.excalidraw.md", false));
        assert!(ig.matches("Daily/2026/09/old-1.md", false));
        assert!(!ig.matches("Daily/2026/new.md", false));
        let ig = Ignore::from_config(&json!({}));
        assert!(!wanted(&ig, "Reflections/Work/Day/2026-09-28.md"));
        assert!(!wanted(&ig, ".obsidian/workspace.json"));
        assert!(!wanted(&ig, "Topics/.T.md.alethiometer-tmp"));
        assert!(wanted(&ig, "Topics/T.md"));
    }
}

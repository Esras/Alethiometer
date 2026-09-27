//! Source sync. Each connector turns a remote API into a `Batch` of normalized
//! records; `apply` writes a batch into SQLite. Connectors never touch the
//! database directly, which keeps them testable with canned HTTP responses.
//!
//! Boundary rules enforced here, not in connectors:
//! - a `personal` source may only produce untitled busy blocks (rule 3);
//! - nothing is ever deleted except a personal source's own busy-block cache,
//!   which is replaced wholesale on each poll (it holds no user content).

pub mod connectors;
pub mod http;

use crate::error::{msg, Result};
use crate::seed::fmt;
use crate::settings::SourceConfig;
use crate::state::App;
use http::Http;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

/// A source row plus its settings, handed to a connector.
#[derive(Debug, Clone)]
pub struct Source {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub config: Value,
    pub cursor: Option<String>,
}

/// What a connector may read while polling or sending.
pub struct Ctx<'a> {
    pub source: &'a Source,
    pub http: &'a dyn Http,
    /// Secrets by name (Keychain on the Mac, 0600 file on Linux).
    pub secret: &'a dyn Fn(&str) -> Option<String>,
    /// Local wall clock (respects fake_now).
    pub now: chrono::NaiveDateTime,
}

impl Ctx<'_> {
    pub fn require_secret(&self, key: &str) -> anyhow::Result<String> {
        (self.secret)(key).ok_or_else(|| anyhow::anyhow!("missing secret '{key}' for source '{}'", self.source.name))
    }

    pub fn config_str(&self, key: &str) -> anyhow::Result<String> {
        self.source
            .config
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("source '{}' needs config.{key}", self.source.name))
    }
}

#[derive(Debug, Default, Clone)]
pub struct Batch {
    pub items: Vec<ItemUpsert>,
    /// For personal calendars: the full set of busy blocks in the synced window.
    /// Replaces this source's previous busy blocks.
    pub busy: Option<Vec<Busy>>,
    /// New cursor to store (sync token, updated-since timestamp, etc.).
    pub cursor: Option<String>,
    /// External ids the remote side cancelled or deleted. The item is kept (no
    /// deletions of user-visible records), marked done and resurfaced as new
    /// signal; only its calendar block, a cache of remote times, is dropped.
    /// Unknown ids are ignored.
    pub cancelled: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ItemUpsert {
    /// Stable id in the remote system. (source, external_id) is unique.
    pub external_id: String,
    /// message | thread | issue | pr | event | note | email | doc | task
    pub kind: String,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    /// Author handle in the source (Slack user id, Jira accountId, GitHub login…).
    pub author: Option<Handle>,
    /// Local time "YYYY-MM-DDTHH:MM:SS".
    pub occurred_at: String,
    pub ended_at: Option<String>,
    pub due_at: Option<String>,
    /// open | done
    pub status: String,
    pub msg_count: Option<i64>,
    pub owner: Option<String>,
    pub raw_json: Value,
    /// Expanded context (e.g. Slack messages since last look). Replaces stored ones.
    pub messages: Vec<Message>,
    /// Set when this is a calendar event.
    pub event: Option<EventFields>,
    /// True when this update is new signal (new messages, reassigned, state change).
    /// New items are always new signal.
    pub bump: bool,
    /// Remove this item's calendar block if it has one (declined, or now all-day).
    /// The item itself stays. Ignored when `event` is set.
    pub drop_event: bool,
}

#[derive(Debug, Clone)]
pub struct Handle {
    pub handle: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub at: String,
    pub author: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct EventFields {
    pub start: String,
    pub end: String,
    /// Category key from settings (e.g. work, self).
    pub category: String,
    pub tentative: bool,
    pub attendees: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Busy {
    pub start: String,
    pub end: String,
}

/// A connector for one source kind.
pub trait Connector: Send + Sync {
    /// Fetch changes since `ctx.source.cursor`.
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch>;

    /// Perform one outbox write. Return a receipt only once the remote side has
    /// confirmed it; any error leaves the row failed and visible.
    fn send(&self, _ctx: &Ctx, op: &str, _payload: &Value) -> anyhow::Result<Value> {
        anyhow::bail!("this source doesn't support '{op}'")
    }
}

/// Make sure every configured source has a row, and sync its tag/config.
pub fn register_sources(conn: &Connection, configs: &[SourceConfig]) -> Result<()> {
    for c in configs {
        if c.tag != "work" && c.tag != "personal" {
            return Err(msg(format!("source '{}': tag must be work or personal", c.name)));
        }
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM sources WHERE kind = ?1 AND name = ?2", params![c.kind, c.name], |r| r.get(0))
            .optional()?;
        match existing {
            Some(id) => conn.execute(
                "UPDATE sources SET tag = ?1, config_json = ?2, enabled = ?3 WHERE id = ?4",
                params![c.tag, c.config.to_string(), c.enabled, id],
            )?,
            None => conn.execute(
                "INSERT INTO sources (kind, tag, name, config_json, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![c.kind, c.tag, c.name, c.config.to_string(), c.enabled],
            )?,
        };
    }
    Ok(())
}

pub fn load_source(conn: &Connection, id: i64) -> Result<Source> {
    Ok(conn.query_row("SELECT id, kind, tag, name, config_json, cursor FROM sources WHERE id = ?1", [id], |r| {
        let config: String = r.get(4)?;
        Ok(Source {
            id: r.get(0)?,
            kind: r.get(1)?,
            tag: r.get(2)?,
            name: r.get(3)?,
            config: serde_json::from_str(&config).unwrap_or(Value::Null),
            cursor: r.get(5)?,
        })
    })?)
}

/// Write a batch. Returns the number of items that became new signal.
pub fn apply(conn: &Connection, source: &Source, batch: &Batch) -> Result<usize> {
    if source.tag == "personal" && (!batch.items.is_empty() || !batch.cancelled.is_empty()) {
        return Err(msg(format!(
            "personal source '{}' tried to store items; only busy blocks are allowed",
            source.name
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let mut new_signal = 0;
    for it in &batch.items {
        let author = match &it.author {
            Some(h) => Some(person_for(&tx, &source.kind, h)?),
            None => None,
        };
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2",
                params![source.id, it.external_id],
                |r| r.get(0),
            )
            .optional()?;
        let item_id = match existing {
            None => {
                tx.execute(
                    "INSERT INTO items (source_id, external_id, kind, title, body, summary, url, author_person_id,
                       occurred_at, ended_at, due_at, status, msg_count, owner, seen, raw_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, ?15)",
                    params![
                        source.id, it.external_id, it.kind, it.title, it.body, it.summary, it.url, author,
                        it.occurred_at, it.ended_at, it.due_at, it.status, it.msg_count, it.owner, it.raw_json.to_string()
                    ],
                )?;
                new_signal += 1;
                tx.last_insert_rowid()
            }
            Some(id) => {
                tx.execute(
                    "UPDATE items SET kind = ?2, title = ?3, body = ?4, summary = ?5, url = ?6, author_person_id = ?7,
                       occurred_at = ?8, ended_at = ?9, due_at = ?10, status = ?11, msg_count = ?12, owner = ?13,
                       raw_json = ?14, seen = CASE WHEN ?15 THEN 0 ELSE seen END
                     WHERE id = ?1",
                    params![
                        id, it.kind, it.title, it.body, it.summary, it.url, author, it.occurred_at, it.ended_at,
                        it.due_at, it.status, it.msg_count, it.owner, it.raw_json.to_string(), it.bump
                    ],
                )?;
                if it.bump {
                    new_signal += 1;
                }
                id
            }
        };
        if !it.messages.is_empty() {
            tx.execute("DELETE FROM item_messages WHERE item_id = ?1", [item_id])?;
            for m in &it.messages {
                tx.execute(
                    "INSERT INTO item_messages (item_id, at, author, text) VALUES (?1, ?2, ?3, ?4)",
                    params![item_id, m.at, m.author, m.text],
                )?;
            }
        }
        if let Some(e) = &it.event {
            let ev: Option<i64> = tx.query_row("SELECT id FROM events WHERE item_id = ?1", [item_id], |r| r.get(0)).optional()?;
            match ev {
                Some(ev) => tx.execute(
                    "UPDATE events SET start = ?2, end = ?3, title = ?4, category = ?5, tentative = ?6, attendees = ?7 WHERE id = ?1",
                    params![ev, e.start, e.end, it.title, e.category, e.tentative, e.attendees],
                )?,
                None => tx.execute(
                    "INSERT INTO events (item_id, source_id, start, end, title, category, tentative, attendees)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![item_id, source.id, e.start, e.end, it.title, e.category, e.tentative, e.attendees],
                )?,
            };
        } else if it.drop_event {
            tx.execute("DELETE FROM events WHERE item_id = ?1 AND is_life_busy = 0", [item_id])?;
        }
    }
    for ext in &batch.cancelled {
        let id: Option<i64> = tx
            .query_row("SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2", params![source.id, ext], |r| r.get(0))
            .optional()?;
        if let Some(id) = id {
            new_signal += tx.execute("UPDATE items SET status = 'done', seen = 0 WHERE id = ?1 AND status != 'done'", [id])?;
            tx.execute("DELETE FROM events WHERE item_id = ?1 AND is_life_busy = 0", [id])?;
        }
    }
    if let Some(busy) = &batch.busy {
        // Keep per-block buffer overrides across refreshes, keyed by start time.
        let mut buffers = std::collections::HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT start, buffer_min FROM events WHERE source_id = ?1 AND buffer_min IS NOT NULL")?;
            for row in stmt.query_map([source.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (s, b) = row?;
                buffers.insert(s, b);
            }
        }
        tx.execute("DELETE FROM events WHERE source_id = ?1 AND is_life_busy = 1", [source.id])?;
        for b in busy {
            tx.execute(
                "INSERT INTO events (source_id, start, end, category, is_life_busy, buffer_min) VALUES (?1, ?2, ?3, 'life', 1, ?4)",
                params![source.id, b.start, b.end, buffers.get(&b.start)],
            )?;
        }
    }
    tx.execute(
        "UPDATE sources SET cursor = COALESCE(?2, cursor), last_sync_at = datetime('now', 'localtime'), last_error = NULL WHERE id = ?1",
        params![source.id, batch.cursor],
    )?;
    tx.commit()?;
    Ok(new_signal)
}

fn person_for(conn: &Connection, kind: &str, h: &Handle) -> Result<i64> {
    if let Some(id) = conn
        .query_row(
            "SELECT person_id FROM person_handles WHERE source_kind = ?1 AND handle = ?2",
            params![kind, h.handle],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute("INSERT INTO people (display_name) VALUES (?1)", [&h.display_name])?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO person_handles (person_id, source_kind, handle) VALUES (?1, ?2, ?3)",
        params![id, kind, h.handle],
    )?;
    Ok(id)
}

/// Poll one source now. Errors are stored on the source row and returned.
pub fn sync_source(app: &App, source_id: i64) -> Result<usize> {
    let source = load_source(&app.db(), source_id)?;
    let connector = connectors::for_kind(&source.kind).ok_or_else(|| msg(format!("no connector for '{}'", source.kind)))?;
    let http = http::UreqHttp::default();
    let secret = |k: &str| app.platform.secret_get(k);
    let ctx = Ctx { source: &source, http: &http, secret: &secret, now: app.clock.now() };
    // Poll without holding the DB lock; network calls can be slow.
    let result = connector.poll(&ctx).map_err(|e| msg(e.to_string())).and_then(|batch| {
        let conn = app.db();
        let n = apply(&conn, &source, &batch)?;
        // Follow-ups that need the database (topic links, closing vanished tasks).
        if source.kind == "obsidian" {
            return Ok(n.saturating_sub(connectors::obsidian::after_apply(&conn, &source, &batch)?));
        }
        // GitHub: items from a repo linked to exactly one topic become suggested links.
        if source.kind == "github" {
            connectors::github::link_repo_topics(&conn, source.id)?;
        }
        Ok(n)
    });
    if let Err(e) = &result {
        app.db().execute("UPDATE sources SET last_error = ?2 WHERE id = ?1", params![source_id, e.to_string()])?;
    }
    result
}

/// Background polling for every enabled source. One thread per source.
pub fn start(app: std::sync::Arc<App>) {
    let ids: Vec<(i64, String)> = {
        let conn = app.db();
        let names: Vec<String> = app.settings.sources.iter().filter(|s| s.enabled).map(|s| s.name.clone()).collect();
        let mut stmt = match conn.prepare("SELECT id, name FROM sources WHERE enabled = 1") {
            Ok(s) => s,
            Err(_) => return,
        };
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)));
        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).filter(|(_, n): &(i64, String)| names.contains(n)).collect(),
            Err(_) => return,
        }
    };
    for (id, name) in ids {
        let cfg = app.settings.sources.iter().find(|s| s.name == name);
        let poll = cfg.map(|s| s.poll_secs).unwrap_or(300).max(30);
        // Slack: live Socket Mode nudges on top of polling (no-op without an app-level token).
        if cfg.is_some_and(|s| s.kind == "slack") {
            connectors::slack::socket::start(app.clone(), id);
        }
        let app = app.clone();
        // The vault runs its own loop: file watcher plus a short poll.
        if cfg.is_some_and(|s| s.kind == "obsidian") {
            connectors::obsidian::run(app, id, name, std::time::Duration::from_secs(poll));
            continue;
        }
        std::thread::Builder::new()
            .name(format!("sync-{name}"))
            .spawn(move || loop {
                if let Err(e) = sync_source(&app, id) {
                    eprintln!("sync {name}: {e}");
                }
                std::thread::sleep(std::time::Duration::from_secs(poll));
            })
            .ok();
    }
}

#[derive(Serialize)]
pub struct SourceStatus {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "status" => {
            let conn = app.db();
            let mut stmt = conn.prepare("SELECT id, kind, tag, name, last_sync_at, last_error FROM sources ORDER BY kind, name")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(SourceStatus {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        tag: r.get(2)?,
                        name: r.get(3)?,
                        last_sync_at: r.get(4)?,
                        last_error: r.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(serde_json::json!(rows))
        }
        "now" => {
            let id = crate::queries::arg_i64(args, "source_id")?;
            let n = sync_source(app, id)?;
            Ok(serde_json::json!({ "new_signals": n }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

/// `YYYY-MM-DDTHH:MM:SS` in local time from an RFC 3339 timestamp.
pub fn local_ts(rfc3339: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|t| fmt(t.with_timezone(&chrono::Local).naive_local()))
}

/// `YYYY-MM-DDTHH:MM:SS` in local time from Unix seconds (Slack's "ts" format works too).
pub fn local_from_unix(secs: f64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|t| fmt(t.with_timezone(&chrono::Local).naive_local()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(tag: &str) -> (Connection, Source) {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = SourceConfig {
            kind: "jira".into(),
            tag: tag.into(),
            name: "Work Jira".into(),
            enabled: true,
            poll_secs: 300,
            config: serde_json::json!({}),
        };
        register_sources(&conn, &[cfg]).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM sources", [], |r| r.get(0)).unwrap();
        let src = load_source(&conn, id).unwrap();
        (conn, src)
    }

    fn issue(bump: bool) -> ItemUpsert {
        ItemUpsert {
            external_id: "PLAT-1".into(),
            kind: "issue".into(),
            title: "Fix retries".into(),
            occurred_at: "2026-09-28T09:00:00".into(),
            status: "open".into(),
            author: Some(Handle { handle: "acc-1".into(), display_name: "Jordan".into() }),
            bump,
            ..Default::default()
        }
    }

    #[test]
    fn upsert_marks_new_then_only_bumps_resurface() {
        let (conn, src) = setup("work");
        let b = Batch { items: vec![issue(false)], cursor: Some("c1".into()), ..Default::default() };
        assert_eq!(apply(&conn, &src, &b).unwrap(), 1);
        conn.execute("UPDATE items SET seen = 1", []).unwrap();
        assert_eq!(apply(&conn, &src, &b).unwrap(), 0);
        let seen: i64 = conn.query_row("SELECT seen FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(seen, 1);
        let b2 = Batch { items: vec![issue(true)], ..Default::default() };
        assert_eq!(apply(&conn, &src, &b2).unwrap(), 1);
        let (seen, n, cursor): (i64, i64, String) = conn
            .query_row("SELECT seen, (SELECT count(*) FROM items), (SELECT cursor FROM sources) FROM items", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((seen, n, cursor.as_str()), (0, 1, "c1"));
        // One person per handle.
        let people: i64 = conn.query_row("SELECT count(*) FROM people", [], |r| r.get(0)).unwrap();
        assert_eq!(people, 1);
    }

    #[test]
    fn personal_source_cannot_store_items() {
        let (conn, src) = setup("personal");
        let b = Batch { items: vec![issue(false)], ..Default::default() };
        assert!(apply(&conn, &src, &b).is_err());
        let n: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn busy_blocks_replace_and_keep_buffer_overrides() {
        let (conn, src) = setup("personal");
        let busy = |s: &str, e: &str| Busy { start: s.into(), end: e.into() };
        let b = Batch { busy: Some(vec![busy("2026-09-28T12:00:00", "2026-09-28T13:00:00")]), ..Default::default() };
        apply(&conn, &src, &b).unwrap();
        conn.execute("UPDATE events SET buffer_min = 25", []).unwrap();
        let b = Batch {
            busy: Some(vec![busy("2026-09-28T12:00:00", "2026-09-28T13:00:00"), busy("2026-09-28T17:30:00", "2026-09-28T18:00:00")]),
            ..Default::default()
        };
        apply(&conn, &src, &b).unwrap();
        let rows: Vec<(String, Option<i64>, Option<String>)> = conn
            .prepare("SELECT start, buffer_min, title FROM events ORDER BY start")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, Some(25));
        assert!(rows.iter().all(|r| r.2.is_none()));
    }

    #[test]
    fn bad_tag_is_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = SourceConfig {
            kind: "jira".into(),
            tag: "home".into(),
            name: "x".into(),
            enabled: true,
            poll_secs: 300,
            config: serde_json::json!({}),
        };
        assert!(register_sources(&conn, &[cfg]).is_err());
    }
}

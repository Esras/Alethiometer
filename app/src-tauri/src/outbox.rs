//! Writeback. Every write to a remote system goes through the `outbox` table:
//! pending → sent → confirmed, or failed(reason). Local state changes only on
//! `confirmed`. Failures stay visible and can be retried (⌘R). Nothing is
//! removed locally until the remote side confirms.

use crate::error::{msg, Result};
use crate::state::App;
use crate::sync::{connectors, http, load_source, Ctx};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

/// Queue a write. Returns the outbox id.
pub fn enqueue(conn: &Connection, source_id: i64, op: &str, payload: &Value, item_id: Option<i64>) -> Result<i64> {
    let tag: String = conn.query_row("SELECT tag FROM sources WHERE id = ?1", [source_id], |r| r.get(0))?;
    if tag == "personal" && op != "marvin.add_task" {
        return Err(msg("writes to personal sources are limited to Phase 2 life lines"));
    }
    conn.execute(
        "INSERT INTO outbox (source_id, op, payload_json, item_id) VALUES (?1, ?2, ?3, ?4)",
        params![source_id, op, payload.to_string(), item_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Apply the local side of a confirmed write.
fn on_confirmed(conn: &Connection, op: &str, item_id: Option<i64>) -> Result<()> {
    if let Some(id) = item_id {
        match op {
            "slack.mark_read" => {
                conn.execute("UPDATE items SET seen = 1 WHERE id = ?1", [id])?;
            }
            "jira.transition" | "github.close" => {
                // The next poll brings the real status; nothing to guess here.
            }
            _ => {}
        }
    }
    Ok(())
}

/// Send one pending row. Returns false when there was nothing to send.
pub fn process_one(app: &App, http: &dyn http::Http) -> Result<bool> {
    let row: Option<(i64, i64, String, String, Option<i64>)> = app
        .db()
        .query_row(
            "SELECT id, source_id, op, payload_json, item_id FROM outbox WHERE state = 'pending' ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((id, source_id, op, payload, item_id)) = row else { return Ok(false) };
    app.db().execute(
        "UPDATE outbox SET state = 'sent', sent_at = datetime('now', 'localtime'), attempts = attempts + 1 WHERE id = ?1",
        [id],
    )?;
    let source = load_source(&app.db(), source_id)?;
    let result = match connectors::for_kind(&source.kind) {
        None => Err(anyhow::anyhow!("no connector for '{}'", source.kind)),
        Some(c) => {
            let secret = |k: &str| app.platform.secret_get(k);
            let ctx = Ctx { source: &source, http, secret: &secret, now: app.clock.now() };
            let payload: Value = serde_json::from_str(&payload)?;
            c.send(&ctx, &op, &payload)
        }
    };
    let conn = app.db();
    match result {
        Ok(receipt) => {
            conn.execute(
                "UPDATE outbox SET state = 'confirmed', confirmed_at = datetime('now', 'localtime'), receipt_json = ?2, error = NULL WHERE id = ?1",
                params![id, receipt.to_string()],
            )?;
            on_confirmed(&conn, &op, item_id)?;
        }
        Err(e) => {
            conn.execute("UPDATE outbox SET state = 'failed', error = ?2 WHERE id = ?1", params![id, e.to_string()])?;
        }
    }
    Ok(true)
}

/// Background sender: drains pending rows, then waits.
pub fn start(app: std::sync::Arc<App>) {
    std::thread::Builder::new()
        .name("outbox".into())
        .spawn(move || {
            let http = http::UreqHttp::default();
            loop {
                match process_one(&app, &http) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(e) => eprintln!("outbox: {e}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        })
        .ok();
}

#[derive(Serialize)]
struct Row {
    id: i64,
    source: String,
    op: String,
    state: String,
    error: Option<String>,
    item_id: Option<i64>,
    created_at: String,
    confirmed_at: Option<String>,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list" => {
            let conn = app.db();
            let mut stmt = conn.prepare(
                "SELECT o.id, s.name, o.op, o.state, o.error, o.item_id, o.created_at, o.confirmed_at
                   FROM outbox o JOIN sources s ON s.id = o.source_id
                  WHERE o.state != 'confirmed' OR o.confirmed_at > datetime('now', 'localtime', '-1 day')
                  ORDER BY o.id DESC LIMIT 200",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Row {
                        id: r.get(0)?,
                        source: r.get(1)?,
                        op: r.get(2)?,
                        state: r.get(3)?,
                        error: r.get(4)?,
                        item_id: r.get(5)?,
                        created_at: r.get(6)?,
                        confirmed_at: r.get(7)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(json!(rows))
        }
        // ⌘R: failed rows go back to pending. Without an id, retry every failed row.
        "retry" => {
            let n = match crate::queries::opt_i64(args, "id") {
                Some(id) => app.db().execute("UPDATE outbox SET state = 'pending' WHERE id = ?1 AND state = 'failed'", [id])?,
                None => app.db().execute("UPDATE outbox SET state = 'pending' WHERE state = 'failed'", [])?,
            };
            Ok(json!({ "retried": n }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

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
fn on_confirmed(conn: &Connection, op: &str, item_id: Option<i64>, receipt: &Value) -> Result<()> {
    if let Some(id) = item_id {
        match op {
            // The Notes line's Incoming item now points at the new Jira issue.
            "jira.create_issue" => {
                let key = receipt.get("key").and_then(Value::as_str);
                let url = receipt.get("url").and_then(Value::as_str);
                if let Some(key) = key {
                    conn.execute(
                        "UPDATE items SET url = COALESCE(?2, url), summary = ?3 WHERE id = ?1",
                        params![id, url, format!("Jira · {key}")],
                    )?;
                }
            }
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
            on_confirmed(&conn, &op, item_id, &receipt)?;
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

#[cfg(test)]
mod integration_tests {
    use crate::settings::Settings;
    use crate::state::App;
    use serde_json::json;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        (app, dir)
    }

    #[test]
    fn confirming_a_slack_signal_queues_mark_read() {
        let (app, _d) = app();
        let conn = app.db();
        let slack: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0)).unwrap();
        let topic: i64 = conn.query_row("SELECT id FROM topics LIMIT 1", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at, raw_json)
             VALUES (?1, 'C1/1', 'thread', '#x', '2026-09-28T09:00:00', ?2)",
            rusqlite::params![slack, json!({ "channel": "C1", "mark_ts": "1727.1" }).to_string()],
        )
        .unwrap();
        let item = conn.last_insert_rowid();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", [item, topic]).unwrap();
        crate::queries::incoming::confirm(&conn, item, topic).unwrap();
        let (op, payload, item_id): (String, String, i64) = conn
            .query_row("SELECT op, payload_json, item_id FROM outbox WHERE op = 'slack.mark_read'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(op, "slack.mark_read");
        assert_eq!(item_id, item);
        assert!(payload.contains("1727.1"));
    }

    #[test]
    fn slack_items_without_a_read_marker_queue_nothing() {
        let (app, _d) = app();
        let conn = app.db();
        let (item, topic): (i64, i64) = conn
            .query_row(
                "SELECT tl.item_id, tl.topic_id FROM topic_links tl JOIN items i ON i.id = tl.item_id
                   JOIN sources s ON s.id = i.source_id WHERE s.kind = 'slack' AND tl.state = 'suggested' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        crate::queries::incoming::confirm(&conn, item, topic).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn confirmed_jira_create_points_the_item_at_the_issue() {
        let (app, _d) = app();
        let conn = app.db();
        let item: i64 = conn.query_row("SELECT id FROM items LIMIT 1", [], |r| r.get(0)).unwrap();
        super::on_confirmed(&conn, "jira.create_issue", Some(item), &json!({ "key": "PLAT-9", "url": "https://x/browse/PLAT-9" })).unwrap();
        let (url, summary): (String, String) =
            conn.query_row("SELECT url, summary FROM items WHERE id = ?1", [item], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(url, "https://x/browse/PLAT-9");
        assert_eq!(summary, "Jira · PLAT-9");
    }
}

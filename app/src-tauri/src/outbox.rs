//! Writeback. Every write to a remote system goes through the `outbox` table:
//! pending → sent → confirmed, or failed(reason). Local state changes only on
//! `confirmed`. Failures stay visible and can be retried (⌘R). Nothing is
//! removed locally until the remote side confirms.

use crate::domain::ports::{Reads, UnitOfWork};
use crate::domain::sources::SourceRow;
use crate::time::Ts;
use crate::error::{msg, Result};
use crate::state::App;
use crate::sync::{connectors, http, Ctx};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

/// Queue a write to a source, if it takes writes. Returns the outbox id.
pub fn request(uow: &dyn UnitOfWork, source_id: i64, op: &str, payload: &Value, item_id: Option<i64>) -> Result<i64> {
    let source = uow.reads().sources().get(source_id)?.ok_or_else(|| msg("no such source"))?;
    if source.tag == "personal" && op != "marvin.add_task" {
        return Err(msg("writes to personal sources are limited to Phase 2 life lines"));
    }
    if let Some(name) = read_only_name(&source) {
        return Err(msg(format!("'{name}' is read-only (read_only in settings.json)")));
    }
    uow.outbox().enqueue(source_id, op, &payload.to_string(), item_id)
}

/// The source's name if its config sets `"read_only": true`.
pub fn read_only_name(source: &SourceRow) -> Option<&str> {
    let config: Value = serde_json::from_str(&source.config_json).unwrap_or(Value::Null);
    config.get("read_only").and_then(Value::as_bool).unwrap_or(false).then_some(source.name.as_str())
}

/// Whether a source takes no writes (`read_only` in its config).
pub fn is_read_only(r: &dyn Reads, source_id: i64) -> Result<bool> {
    Ok(r.sources().get(source_id)?.as_ref().is_some_and(|s| read_only_name(s).is_some()))
}

/// `request` for code not moved to units of work yet.
pub fn enqueue(conn: &Connection, source_id: i64, op: &str, payload: &Value, item_id: Option<i64>) -> Result<i64> {
    request(&crate::store::over(conn), source_id, op, payload, item_id)
}

/// Name of the source if its config sets `"read_only": true`.
pub fn read_only(conn: &Connection, source_id: i64) -> Result<Option<String>> {
    let source = Reads::sources(&crate::store::over(conn)).get(source_id)?.ok_or_else(|| msg("no such source"))?;
    Ok(read_only_name(&source).map(str::to_owned))
}

/// Apply the local side of a confirmed write.
fn on_confirmed(uow: &dyn UnitOfWork, now: Ts, op: &str, item_id: Option<i64>, payload: &Value, receipt: &Value) -> Result<()> {
    if op == "jira.create_issue" && payload.get("assign_me").and_then(Value::as_bool) == Some(true) {
        crate::queries::todos::issue_created(uow, now, payload, receipt)?;
    }
    if let (Some(id), Some(done)) = (item_id, payload.get("done").and_then(Value::as_bool)) {
        if op == "jira.transition" || op == "obsidian.toggle_task" {
            uow.items().set_status(id, if done { "done" } else { "open" })?;
        }
    }
    if let Some(id) = item_id {
        match op {
            // The Notes line's Incoming item now points at the new Jira issue.
            "jira.create_issue" => {
                if let Some(key) = receipt.get("key").and_then(Value::as_str) {
                    uow.items().set_url_and_summary(id, receipt.get("url").and_then(Value::as_str), &format!("Jira · {key}"))?;
                }
            }
            // Nothing local: Confirm already took the item out of Incoming, and a
            // reply that came in since (it set `seen = 0` again) must stay there.
            "slack.mark_read" | "jira.transition" | "github.close" => {
                // The next poll brings the real status; nothing to guess here.
            }
            _ => {}
        }
    }
    Ok(())
}

/// Send one pending row: claimed in one unit of work, sent with no database
/// access, and its outcome (with the local side of a confirmed write)
/// recorded in another. Returns false when there was nothing to send.
pub fn process_one(app: &App, http: &dyn http::Http) -> Result<bool> {
    let claimed = app.store().write(|uow| {
        let Some(row) = uow.outbox().claim_next()? else { return Ok(None) };
        let source = crate::sync::source(uow.reads(), row.source_id)?;
        // Rows queued straight into the table (not via `request`) still respect read-only.
        let blocked = uow.reads().sources().get(row.source_id)?.as_ref().and_then(|s| read_only_name(s).map(str::to_owned));
        Ok(Some((row, source, blocked)))
    })?;
    let Some((row, source, blocked)) = claimed else { return Ok(false) };
    let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or_default();
    let result = match connectors::for_kind(&source.kind) {
        _ if blocked.is_some() => Err(anyhow::anyhow!("'{}' is read-only (read_only in settings.json)", source.name)),
        None => Err(anyhow::anyhow!("no connector for '{}'", source.kind)),
        Some(c) => {
            let secret = |k: &str| app.platform.secret_get(k);
            let ctx = Ctx { source: &source, http, secret: &secret, now: app.clock.now(), clock: app.clock.clone() };
            c.send(&ctx, &row.op, &payload)
        }
    };
    let now = app.clock.now();
    app.store().write(|uow| match &result {
        Ok(receipt) => {
            uow.outbox().confirm(row.id, &receipt.to_string())?;
            on_confirmed(uow, now, &row.op, row.item_id, &payload, receipt)
        }
        Err(e) => uow.outbox().fail(row.id, &e.to_string()),
    })?;
    Ok(true)
}

static WAKE: (std::sync::Mutex<bool>, std::sync::Condvar) = (std::sync::Mutex::new(false), std::sync::Condvar::new());

/// Send what's pending now rather than at the next 5 s tick (e.g. an edit the
/// user is watching save).
pub fn wake() {
    *WAKE.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    WAKE.1.notify_all();
}

/// Background sender: drains pending rows, then waits (5 s, or until `wake`).
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
                let woken = WAKE.0.lock().unwrap_or_else(|e| e.into_inner());
                let (mut woken, _) = WAKE
                    .1
                    .wait_timeout_while(woken, std::time::Duration::from_secs(5), |w| !*w)
                    .unwrap_or_else(|e| e.into_inner());
                *woken = false;
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
    created_at: Ts,
    confirmed_at: Option<Ts>,
}

pub fn route(name: &str) -> Option<crate::api::Route> {
    use crate::api::Route;
    Some(match name {
        "list" => Route::Read(|r, ctx, _| {
            let since = ctx.clock.now() - jiff::SignedDuration::from_hours(24);
            let rows: Vec<Row> = r
                .outbox()
                .recent(since, 200)?
                .into_iter()
                .map(|e| Row {
                    id: e.id,
                    source: e.source_name,
                    op: e.op,
                    state: e.state,
                    error: e.error,
                    item_id: e.item_id,
                    created_at: e.created_at,
                    confirmed_at: e.confirmed_at,
                })
                .collect();
            Ok(json!(rows))
        }),
        // ⌘R: failed rows go back to pending. Without an id, retry every failed row.
        "retry" => Route::Write(|uow, _, args| Ok(json!({ "retried": uow.outbox().retry(crate::queries::opt_i64(args, "id"))? }))),
        _ => return None,
    })
}

#[cfg(test)]
pub fn tests_on_confirmed(conn: &Connection, op: &str, payload: &Value, receipt: &Value) {
    on_confirmed(&crate::store::over(conn), crate::time::Ts::new(jiff::Timestamp::now()), op, None, payload, receipt).unwrap();
}

#[cfg(test)]
pub fn tests_on_confirmed_item(conn: &Connection, op: &str, item_id: i64, payload: &Value) {
    on_confirmed(&crate::store::over(conn), crate::time::Ts::new(jiff::Timestamp::now()), op, Some(item_id), payload, &json!({})).unwrap();
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
        crate::queries::incoming::confirm(&crate::store::over(&conn), crate::time::Ts::new(jiff::Timestamp::now()), item, topic).unwrap();
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
    fn a_reply_after_confirm_stays_when_the_mark_read_lands() {
        let (app, _d) = app();
        let conn = app.db();
        let slack: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at, seen, raw_json) VALUES (?1, 'C1/2', 'thread', '#x', '2026-09-28T09:00:00', 1, '{}')",
            [slack],
        )
        .unwrap();
        let item = conn.last_insert_rowid();
        // Confirmed (seen = 1); then a thread reply arrives before the outbox sends.
        conn.execute("UPDATE items SET seen = 0 WHERE id = ?1", [item]).unwrap();
        super::on_confirmed(&crate::store::over(&conn), crate::time::t("2026-10-03T09:00:00"), "slack.mark_read", Some(item), &json!({ "channel": "C1", "ts": "1727.1" }), &json!({})).unwrap();
        let seen: i64 = conn.query_row("SELECT seen FROM items WHERE id = ?1", [item], |r| r.get(0)).unwrap();
        assert_eq!(seen, 0, "the reply is still in Incoming");
    }

    #[test]
    fn confirming_on_a_read_only_slack_source_queues_nothing_and_succeeds() {
        let (app, _d) = app();
        let conn = app.db();
        let slack: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'slack'", [], |r| r.get(0)).unwrap();
        conn.execute("UPDATE sources SET config_json = json_set(config_json, '$.read_only', json('true')) WHERE id = ?1", [slack]).unwrap();
        let topic: i64 = conn.query_row("SELECT id FROM topics LIMIT 1", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at, raw_json) VALUES (?1, 'C1/3', 'thread', '#x', '2026-09-28T09:00:00', ?2)",
            rusqlite::params![slack, json!({ "channel": "C1", "mark_ts": "1727.1" }).to_string()],
        )
        .unwrap();
        let item = conn.last_insert_rowid();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')", [item, topic]).unwrap();
        crate::queries::incoming::confirm(&crate::store::over(&conn), crate::time::Ts::new(jiff::Timestamp::now()), item, topic).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE item_id = ?1", [item], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
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
        crate::queries::incoming::confirm(&crate::store::over(&conn), crate::time::Ts::new(jiff::Timestamp::now()), item, topic).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM outbox WHERE op = 'slack.mark_read'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn confirmed_jira_create_points_the_item_at_the_issue() {
        let (app, _d) = app();
        let conn = app.db();
        let item: i64 = conn.query_row("SELECT id FROM items LIMIT 1", [], |r| r.get(0)).unwrap();
        super::on_confirmed(&crate::store::over(&conn), crate::time::t("2026-10-03T09:00:00"), "jira.create_issue", Some(item), &json!({}), &json!({ "key": "PLAT-9", "url": "https://x/browse/PLAT-9" })).unwrap();
        let (url, summary): (String, String) =
            conn.query_row("SELECT url, summary FROM items WHERE id = ?1", [item], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(url, "https://x/browse/PLAT-9");
        assert_eq!(summary, "Jira · PLAT-9");
    }
}

#[cfg(test)]
mod read_only_tests {
    use serde_json::json;

    #[test]
    fn read_only_sources_refuse_writes() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources (kind, tag, name, config_json) VALUES ('github', 'work', 'Work GitHub', ?1)",
            [json!({ "read_only": true }).to_string()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        let err = super::enqueue(&conn, id, "github.comment", &json!({}), None).unwrap_err().to_string();
        assert!(err.contains("read-only"), "{err}");
        conn.execute("UPDATE sources SET config_json = '{}' WHERE id = ?1", [id]).unwrap();
        assert!(super::enqueue(&conn, id, "github.comment", &json!({}), None).is_ok());
    }
}

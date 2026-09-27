//! ⌘K search over items (FTS5) and topics.

use super::arg_str;
use crate::api::unknown;
use crate::error::Result;
use crate::state::App;
use serde_json::{json, Value};

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "query" => query(app, &arg_str(args, "q")?),
        _ => Err(unknown(name)),
    }
}

/// Quote each word so user input can't be parsed as FTS syntax; prefix-match the last.
fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .filter(|w| w.len() > 2)
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(format!("{}*", words.join(" ")))
}

fn query(app: &App, q: &str) -> Result<Value> {
    let conn = app.db();
    let like = format!("%{}%", q.trim());
    let topics = conn
        .prepare("SELECT id, name, category FROM topics WHERE archived_at IS NULL AND name LIKE ?1 ORDER BY name LIMIT 5")?
        .query_map([&like], |r| {
            Ok(json!({ "type": "topic", "id": r.get::<_, i64>(0)?, "title": r.get::<_, String>(1)?, "category": r.get::<_, String>(2)? }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let items = match fts_query(q) {
        None => vec![],
        Some(fq) => conn
            .prepare(
                "SELECT i.id, i.title, s.kind, i.occurred_at,
                        (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong' LIMIT 1)
                   FROM items_fts f JOIN items i ON i.id = f.rowid JOIN sources s ON s.id = i.source_id
                  WHERE items_fts MATCH ?1 AND s.tag = 'work'
                  ORDER BY rank LIMIT 20",
            )?
            .query_map([fq], |r| {
                Ok(json!({
                    "type": "item",
                    "id": r.get::<_, i64>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "source": r.get::<_, String>(2)?,
                    "at": r.get::<_, String>(3)?,
                    "topic_id": r.get::<_, Option<i64>>(4)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(json!({ "topics": topics, "items": items }))
}

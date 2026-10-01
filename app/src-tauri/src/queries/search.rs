//! ⌘K search over items (FTS5) and topics, and the full Search results view
//! (18d, `results`): each hit with its source, a snippet with the matches
//! marked, its topic (or none) and its date.

use super::arg_str;
use crate::api::unknown;
use crate::error::Result;
use crate::state::App;
use serde_json::{json, Value};

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "query" => query(app, &arg_str(args, "q")?),
        "results" => results(app, &arg_str(args, "q")?),
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

/// Marks around a match in `snippet`, split out by the UI.
pub const HIT_START: char = '\u{1}';
pub const HIT_END: char = '\u{2}';

/// The Search results view (18d): up to 100 work items, best match first.
/// `snippet` marks matches with `HIT_START`/`HIT_END`; `topic` is the item's
/// confirmed topic (else a suggested one), `msgs` its top-level message count
/// (the split review needs two).
fn results(app: &App, q: &str) -> Result<Value> {
    let Some(fq) = fts_query(q) else { return Ok(json!({ "q": q, "results": [] })) };
    let now = app.clock.now();
    let conn = app.db();
    let mut stmt = conn.prepare(
        "WITH hits AS (
           SELECT rowid AS id, rank, snippet(items_fts, 1, char(1), char(2), '…', 14) AS snip
             FROM items_fts WHERE items_fts MATCH ?1 ORDER BY rank LIMIT 300)
         SELECT i.id, i.title, s.kind, coalesce(i.ended_at, i.occurred_at), h.snip, coalesce(i.summary, ''),
                t.id, t.name, t.category, tl.state,
                (SELECT count(*) FROM item_messages m WHERE m.item_id = i.id AND m.context = 0 AND m.reply_to IS NULL)
           FROM hits h JOIN items i ON i.id = h.id JOIN sources s ON s.id = i.source_id
           LEFT JOIN topic_links tl ON tl.rowid = (
                SELECT x.rowid FROM topic_links x JOIN topics y ON y.id = x.topic_id
                 WHERE x.item_id = i.id AND x.state != 'wrong' AND y.archived_at IS NULL
                 ORDER BY x.state = 'confirmed' DESC, x.updated_at DESC LIMIT 1)
           LEFT JOIN topics t ON t.id = tl.topic_id
          WHERE s.tag = 'work'
          ORDER BY h.rank LIMIT 100",
    )?;
    let rows = stmt
        .query_map([&fq], |r| {
            let at: String = r.get(3)?;
            let snippet: String = r.get(4)?;
            let summary: String = r.get(5)?;
            let topic: Option<i64> = r.get(6)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "title": r.get::<_, String>(1)?,
                "source": r.get::<_, String>(2)?,
                "when": super::parse_ts(&at).map(|t| super::when_label(t, now, false)).unwrap_or_default(),
                "at": at,
                "snippet": if snippet.contains(HIT_START) || summary.is_empty() { snippet } else { summary },
                "topic": match topic {
                    Some(id) => json!({ "id": id, "name": r.get::<_, String>(7)?, "category": r.get::<_, String>(8)?, "state": r.get::<_, String>(9)? }),
                    None => Value::Null,
                },
                "msgs": r.get::<_, i64>(10)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({ "q": q, "results": rows }))
}

#[cfg(test)]
mod tests {
    use crate::ai::context::tests::app;
    use serde_json::json;

    #[test]
    fn results_mark_matches_and_carry_their_topic() {
        let (app, _dir) = app();
        let id: i64 = app.db().query_row("SELECT id FROM items WHERE title = '#inc-2291'", [], |r| r.get(0)).unwrap();
        app.db().execute("UPDATE items SET body = 'Root cause looks like a retry storm from the v2 workers' WHERE id = ?1", [id]).unwrap();
        let v = crate::api::call(&app, "search.results", &json!({ "q": "retry storm" })).unwrap();
        let rs = v["results"].as_array().unwrap();
        let hit = rs.iter().find(|r| r["id"] == id).expect("the incident thread");
        assert!(hit["snippet"].as_str().unwrap().contains("\u{1}retry\u{2}"), "{hit}");
        assert_eq!(hit["topic"]["name"], "Acme outage");
        assert_eq!(hit["topic"]["state"], "suggested");
        assert_eq!(hit["msgs"], 4);
        let nothing = crate::api::call(&app, "search.results", &json!({ "q": "Renewal" })).unwrap();
        let filed = nothing["results"].as_array().unwrap().iter().find(|r| r["topic"]["state"] == "confirmed");
        assert!(filed.is_some(), "filed items carry their confirmed topic");
        let none = crate::api::call(&app, "search.results", &json!({ "q": "  " })).unwrap();
        assert!(none["results"].as_array().unwrap().is_empty(), "nothing to search for");
    }
}

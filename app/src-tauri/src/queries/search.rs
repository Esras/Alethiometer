//! ⌘K search over items (FTS5) and topics, and the full Search results view
//! (18d, `results`): each hit with its source, a snippet with the matches
//! marked, its topic (or none) and its date.

use super::{arg_i64, arg_str};
use crate::api::{unknown, Ctx, Route};
use crate::domain::ports::Reads;
use crate::error::Result;
use crate::state::App;
use serde_json::{json, Value};

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "query" => Route::Read(|r, _, args| query(r, &arg_str(args, "q")?)),
        "results" => Route::Read(|r, ctx, args| results(r, ctx, &arg_str(args, "q")?)),
        "feedback" => Route::Write(|uow, _, args| {
            let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            crate::glean::feedback(
                uow,
                arg_i64(args, "topic_id")?,
                &arg_str(args, "key")?,
                &arg_str(args, "verdict")?,
                &s("title"),
                &s("url"),
                &s("source"),
            )?;
            Ok(json!({ "ok": true }))
        }),
        _ => return None,
    })
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "hydrated" => crate::glean::hydrate::call(app, name, args),
        _ => Err(unknown(name)),
    }
}

/// Quote each word so user input can't be parsed as FTS syntax; prefix-match the last.
pub fn fts_query(q: &str) -> Option<String> {
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

fn query(r: &dyn Reads, q: &str) -> Result<Value> {
    let topics: Vec<Value> = r
        .search()
        .topics_named(q, 5)?
        .into_iter()
        .map(|t| json!({ "type": "topic", "id": t.id, "title": t.name, "category": t.category }))
        .collect();
    let items: Vec<Value> = match fts_query(q) {
        None => vec![],
        Some(fq) => r
            .search()
            .quick(&fq, 20)?
            .into_iter()
            .map(|h| json!({ "type": "item", "id": h.id, "title": h.title, "source": h.source_kind, "at": h.at, "topic_id": h.topic_id }))
            .collect(),
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
fn results(r: &dyn Reads, ctx: &Ctx, q: &str) -> Result<Value> {
    let Some(fq) = fts_query(q) else {
        return Ok(json!({ "q": q, "results": [] }));
    };
    let rows: Vec<Value> = r
        .search()
        .results(&fq, 100)?
        .into_iter()
        .map(|h| {
            json!({
                "id": h.id,
                "title": h.title,
                "source": h.source_kind,
                "when": super::ts_label(h.at, ctx.clock, false),
                "at": h.at,
                "snippet": if h.snippet.contains(HIT_START) || h.summary.is_empty() { h.snippet } else { h.summary },
                "topic": h.topic.map(|t| json!({ "id": t.id, "name": t.name, "category": t.category, "state": t.state })),
                "msgs": h.msgs,
            })
        })
        .collect();
    Ok(json!({ "q": q, "results": rows }))
}

#[cfg(test)]
mod tests {
    use crate::ai::context::tests::app;
    use serde_json::json;

    #[test]
    fn results_mark_matches_and_carry_their_topic() {
        let (app, _dir) = app();
        let id: i64 = app
            .db()
            .query_row("SELECT id FROM items WHERE title = '#inc-2291'", [], |r| r.get(0))
            .unwrap();
        app.db()
            .execute(
                "UPDATE items SET body = 'Root cause looks like a retry storm from the v2 workers' WHERE id = ?1",
                [id],
            )
            .unwrap();
        let v = crate::api::call(&app, "search.results", &json!({ "q": "retry storm" })).unwrap();
        let rs = v["results"].as_array().unwrap();
        let hit = rs.iter().find(|r| r["id"] == id).expect("the incident thread");
        assert!(hit["snippet"].as_str().unwrap().contains("\u{1}retry\u{2}"), "{hit}");
        assert_eq!(hit["topic"]["name"], "Acme outage");
        assert_eq!(hit["topic"]["state"], "suggested");
        assert_eq!(hit["msgs"], 4);
        let nothing = crate::api::call(&app, "search.results", &json!({ "q": "Renewal" })).unwrap();
        let filed = nothing["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["topic"]["state"] == "confirmed");
        assert!(filed.is_some(), "filed items carry their confirmed topic");
        let none = crate::api::call(&app, "search.results", &json!({ "q": "  " })).unwrap();
        assert!(none["results"].as_array().unwrap().is_empty(), "nothing to search for");
    }
}

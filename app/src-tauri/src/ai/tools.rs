//! Tools the ask's model calls when its prompt's overview isn't enough. Work data only (rule 4), as in `context`.

use super::context;
use crate::error::Result;
use crate::state::App;
use jiff::ToSpan;
use serde_json::{json, Value};
use std::fmt::Write;

/// Most tool calls in one answer.
pub const MAX_CALLS: usize = 8;
/// Most characters of tool output in one answer.
pub const MAX_CHARS: usize = 60_000;

/// A tool as the model sees it: every parameter is required.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// (name, JSON type, description)
    pub params: &'static [(&'static str, &'static str, &'static str)],
}

impl ToolDef {
    pub fn schema(&self) -> Value {
        let props: serde_json::Map<String, Value> = self
            .params
            .iter()
            .map(|(n, t, d)| (n.to_string(), json!({ "type": t, "description": d })))
            .collect();
        let required: Vec<&str> = self.params.iter().map(|p| p.0).collect();
        json!({ "type": "object", "properties": props, "required": required })
    }
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

pub const ASK_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "get_item",
        description: "Read one work item in full: its body and up to 40 of its latest messages.",
        params: &[("item_id", "integer", "The number in [item N].")],
    },
    ToolDef {
        name: "get_day",
        description: "The work calendar and the work items touched on one day, in detail.",
        params: &[("date", "string", "The day, YYYY-MM-DD.")],
    },
    ToolDef {
        name: "search",
        description: "Find work items whose text has all of these words (the last may be a prefix), best match first.",
        params: &[(
            "query",
            "string",
            "A few distinctive words: names, projects, ticket keys.",
        )],
    },
];

pub const ASK_TOOLS_PROMPT: &str = "The context is an overview. When the answer needs more than it shows \
(a whole day, an item's full thread, something not listed), call the tools before answering: get_item, get_day, search. \
You have at most 8 calls; use them where they change the answer, and say what you couldn't check.";

/// Run one call; returns its text and the items it showed.
pub fn run(app: &App, call: &ToolCall) -> Result<(String, Vec<i64>)> {
    let int = |k: &str| {
        call.input
            .get(k)
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
    };
    let text = |k: &str| {
        call.input
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    match call.name.as_str() {
        "get_item" => {
            let Some(id) = int("item_id") else {
                return Ok(("item_id must be a number".into(), vec![]));
            };
            Ok(match app.store().read(|r| context::item_detail(r, &app.clock, id))? {
                Some(t) => (t, vec![id]),
                None => (format!("No work item {id}."), vec![]),
            })
        }
        "get_day" => {
            let Ok(day) = crate::queries::parse_date(&text("date")) else {
                return Ok(("date must be YYYY-MM-DD".into(), vec![]));
            };
            let today = app.clock.today();
            let ctx = app
                .store()
                .read(|r| context::for_days(r, &app.clock, today, day, day + 1.days()))?;
            Ok((ctx.text, ctx.used))
        }
        "search" => {
            let Some(fq) = crate::queries::search::fts_query(&text("query")) else {
                return Ok(("Give at least one word of three letters or more.".into(), vec![]));
            };
            let hits = app.store().read(|r| r.search().quick(&fq, 20))?;
            if hits.is_empty() {
                return Ok(("No work items match.".into(), vec![]));
            }
            let mut out = String::new();
            for h in &hits {
                let at = app.clock.local(h.at).strftime("%Y-%m-%d %H:%M");
                let _ = writeln!(
                    out,
                    "- [item {}] {} {at}: {}",
                    h.id,
                    h.source_kind,
                    context::clip(&h.title, 160)
                );
            }
            Ok((out, hits.iter().map(|h| h.id).collect()))
        }
        other => Ok((format!("No tool named {other}."), vec![])),
    }
}

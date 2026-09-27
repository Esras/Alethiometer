//! One entry point for the UI: `call(cmd, args)`. Used by the Tauri command and
//! by the browser dev server, so both behave the same.
//!
//! Commands are `<area>.<name>`; each area module owns its own `call`.

use crate::error::{msg, Result};
use crate::queries;
use crate::state::App;
use serde_json::{json, Value};

pub fn call(app: &App, cmd: &str, args: &Value) -> Result<Value> {
    let (area, name) = cmd.split_once('.').ok_or_else(|| msg(format!("bad command '{cmd}'")))?;
    match area {
        "app" => app_call(app, name),
        "daystrip" => queries::daystrip::call(app, name, args),
        "topics" => queries::topics::call(app, name, args),
        "incoming" => queries::incoming::call(app, name, args),
        "priorities" => queries::priorities::call(app, name, args),
        "notes" => queries::notes::call(app, name, args),
        "reflect" => queries::reflect::call(app, name, args),
        "search" => queries::search::call(app, name, args),
        "sync" => crate::sync::call(app, name, args),
        "outbox" => crate::outbox::call(app, name, args),
        "gcal" => crate::sync::connectors::gcal::auth_call(app, name, args),
        "ai" => match name {
            "ask" => {
                let q = queries::arg_str(args, "question")?;
                Ok(json!({ "answer": crate::ai::ask(&app.settings.ai, &q)? }))
            }
            _ => Err(unknown(cmd)),
        },
        _ => Err(unknown(cmd)),
    }
}

pub fn unknown(cmd: &str) -> crate::error::Error {
    msg(format!("unknown command '{cmd}'"))
}

fn app_call(app: &App, name: &str) -> Result<Value> {
    match name {
        "info" => {
            let incoming: i64 = app.db().query_row(
                "SELECT count(*) FROM items i WHERE i.seen = 0",
                [],
                |r| r.get(0),
            )?;
            Ok(json!({
                "categories": app.settings.categories,
                "day_start_hour": app.settings.day_start_hour,
                "day_end_hour": app.settings.day_end_hour,
                "life_buffer_min": app.settings.life_buffer_min,
                "clock_offset_ms": app.clock.offset_ms(),
                "ai_label": crate::ai::label(&app.settings.ai),
                "incoming_count": incoming,
                "platform": std::env::consts::OS,
            }))
        }
        _ => Err(unknown(name)),
    }
}

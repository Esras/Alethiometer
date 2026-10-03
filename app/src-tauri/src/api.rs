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
        "app" => app_call(app, name, args),
        "agents" => crate::agents::call(app, name, args),
        "daystrip" => queries::daystrip::call(app, name, args),
        "topics" => queries::topics::call(app, name, args),
        "categories" => queries::categories::call(app, name, args),
        "incoming" => queries::incoming::call(app, name, args),
        "priorities" => queries::priorities::call(app, name, args),
        "todos" => queries::todos::call(app, name, args),
        "notes" => queries::notes::call(app, name, args),
        "people" => queries::people::call(app, name, args),
        "reflect" => queries::reflect::call(app, name, args),
        "search" => queries::search::call(app, name, args),
        "sync" => crate::sync::call(app, name, args),
        "settings" => crate::settings::api::call(app, name, args),
        "embed" => crate::embed::call(app, name, args),
        "outbox" => crate::outbox::call(app, name, args),
        "gcal" => crate::sync::connectors::gcal::auth_call(app, name, args),
        "ai" => {
            let out = crate::ai::call(app, name, args);
            if let Err(e) = &out {
                eprintln!("ai.{name} failed: {e}");
            }
            out
        }
        _ => Err(unknown(cmd)),
    }
}

pub fn unknown(cmd: &str) -> crate::error::Error {
    msg(format!("unknown command '{cmd}'"))
}

fn app_call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "info" => {
            let settings = app.settings();
            let incoming: i64 = app.db().query_row(
                "SELECT count(*) FROM items i WHERE i.seen = 0",
                [],
                |r| r.get(0),
            )?;
            Ok(json!({
                "categories": queries::categories::map(&app.db())?,
                "day_start_hour": app.settings().day_start_hour,
                "day_end_hour": app.settings().day_end_hour,
                "life_buffer_min": app.settings().life_buffer_min,
                "quiet_days": app.settings().quiet_days,
                "clock_offset_ms": app.clock.offset_ms(),
                "ai_label": crate::ai::label(&app.settings().ai),
                "incoming_count": incoming,
                "todo_count": queries::todos::list(&app.db(), app.clock.now())?.len(),
                "platform": std::env::consts::OS,
                "ui_scale": settings.ui_scale,
                "ui_font": settings.ui_font,
                "mono_font": settings.mono_font,
                "ui_font_size": settings.ui_font_size,
                "reading_font_size": settings.reading_font_size,
                "settings_rev": app.settings_state().rev,
                // The Assistant searches Glean first when a Glean source is on.
                "glean": crate::glean::config(app).is_some(),
                // How the Agents tab looks (Settings › Agents), and the Needs you count for its tab.
                "agents": { "colors": settings.agents.colors, "names": settings.agents.names, "boxes": settings.agents.boxes },
                "agents_waiting": app.agents.store.lock().requests.len(),
            }))
        }
        "open_url" => {
            let url = args.get("url").and_then(Value::as_str).ok_or_else(|| msg("open_url needs a 'url'"))?;
            let url = &crate::sync::connectors::slack::deep_link(&app.db(), url).unwrap_or_else(|| url.to_owned());
            app.platform.open_url(url).map_err(|e| msg(format!("couldn't open {url}: {e}")))?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(unknown(name)),
    }
}

#[cfg(test)]
mod tests {
    use crate::ai::context::tests::app;

    #[test]
    fn open_url_needs_a_url_arg() {
        let (app, _dir) = app();
        let e = crate::api::call(&app, "app.open_url", &serde_json::json!({})).unwrap_err().to_string();
        assert!(e.contains("needs a 'url'"), "{e}");
    }
}

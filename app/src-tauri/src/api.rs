//! One entry point for the UI: `call(cmd, args)`. Used by the Tauri command and
//! by the browser dev server, so both behave the same.
//!
//! Commands are `<area>.<name>`; each area module owns its own `call`.

//! This is also the middleware between the UI and the database: a command
//! routed as [`Route::Read`] runs on one snapshot, one routed as
//! [`Route::Write`] in one unit of work that commits if it returns `Ok` and
//! rolls back otherwise (`store::Db`). Services never open or commit either.

use crate::clock::Clock;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::queries;
use crate::settings::Settings;
use crate::state::App;
use serde_json::{json, Value};

/// What a service may use besides storage.
pub struct Ctx<'a> {
    pub clock: &'a Clock,
    pub settings: &'a Settings,
}

/// How a command runs, which decides what the middleware opens for it.
pub enum Route {
    /// Reads only, on one consistent snapshot.
    Read(fn(&dyn Reads, &Ctx, &Value) -> Result<Value>),
    /// Changes, in one unit of work.
    Write(fn(&dyn UnitOfWork, &Ctx, &Value) -> Result<Value>),
    /// Reads the database in short snapshots and vault files between them,
    /// with nothing open while it reads a file. No network, no writes: like
    /// `Read`, it can be a live query (`crate::live`), which also re-runs it
    /// when the vault changes.
    ReadFiles(fn(&App, &Value) -> Result<Value>),
}

/// How `cmd` runs, if it's routed (read or write) rather than a module's own `call`.
pub fn route_of(cmd: &str) -> Option<Route> {
    let (area, name) = cmd.split_once('.')?;
    match area {
        "daystrip" => queries::daystrip::route(name),
        "incoming" => queries::incoming::route(name),
        "topics" => queries::topics::route(name),
        "search" => queries::search::route(name),
        "categories" => queries::categories::route(name),
        "people" => queries::people::route(name),
        "priorities" => queries::priorities::route(name),
        "sync" => crate::sync::route(name),
        "outbox" => crate::outbox::route(name),
        "todos" => queries::todos::route(name),
        "reflect" => queries::reflect::route(name),
        _ => None,
    }
}

pub fn call(app: &App, cmd: &str, args: &Value) -> Result<Value> {
    let (area, name) = cmd.split_once('.').ok_or_else(|| msg(format!("bad command '{cmd}'")))?;
    if let Some(route) = route_of(cmd) {
        let settings = app.settings();
        let ctx = Ctx { clock: &app.clock, settings: &settings };
        return match route {
            Route::Read(f) => app.store().read(|r| f(r, &ctx, args)),
            Route::ReadFiles(f) => f(app, args),
            Route::Write(f) => {
                let out = app.store().write(|uow| f(uow, &ctx, args))?;
                // Anything it queued goes out now rather than at the next tick.
                crate::outbox::wake();
                Ok(out)
            }
        };
    }
    // Commands not moved to services yet, and ones that call out (network,
    // files) and open their own short reads and writes around it.
    match area {
        "app" => app_call(app, name, args),
        "live" => crate::live::call(app, name, args),
        "agents" => crate::agents::call(app, name, args),
        "topics" => queries::topics::call(app, name, args),
        "categories" => queries::categories::call(app, name, args),
        "notes" => queries::notes::call(app, name, args),
        "people" => queries::people::call(app, name, args),
        "reflect" => queries::reflect::call(app, name, args),
        "search" => queries::search::call(app, name, args),
        "sync" => crate::sync::call(app, name, args),
        "settings" => crate::settings::api::call(app, name, args),
        "embed" => crate::embed::call(app, name, args),
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
            let (categories, incoming, todos) = app
                .store()
                .read(|r| Ok((queries::categories::map(r)?, r.incoming().unseen_count()?, queries::todos::list(r, &app.clock)?.len())))?;
            Ok(json!({
                "categories": categories,
                "day_start_hour": app.settings().day_start_hour,
                "day_end_hour": app.settings().day_end_hour,
                "life_buffer_min": app.settings().life_buffer_min,
                "quiet_days": app.settings().quiet_days,
                "clock_offset_ms": app.clock.offset_ms(),
                "tz": app.clock.tz().iana_name(),
                "ai_label": crate::ai::label(&app.settings().ai),
                "incoming_count": incoming,
                "todo_count": todos,
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
            let url = &app.store().read(|r| Ok(crate::sync::connectors::slack::deep_link(r, url)))?.unwrap_or_else(|| url.to_owned());
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

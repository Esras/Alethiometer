//! `settings.*` commands for the Settings modal.
//!
//! - `get`: the page (values, rows, sources with status and secrets, header state)
//! - `patch {edits: [{op?, path, value?}]}`: change settings.json and apply it
//! - `state`: the header line and `rev` (polled while the modal is open)
//! - `secret_set {name, value}`: Replace…; the value is never read back
//! - `fonts`: installed font families for the Type pickers
//! - `slack_channels {source}`: channels you're in, for Review…
//! - `open_file`: open settings.json in the default editor (⌘⇧,)

use super::live::{self, Edit};
use super::{schema, sources};
use crate::error::{msg, Result};
use crate::queries::{arg_i64, arg_str};
use crate::state::App;
use serde_json::{json, Value};

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "get" => get(app),
        "state" => Ok(json!(*app.settings_state())),
        "patch" => {
            let edits = args.get("edits").and_then(Value::as_array).ok_or_else(|| msg("patch needs 'edits'"))?;
            let edits = edits.iter().map(Edit::parse).collect::<Result<Vec<_>>>()?;
            Ok(json!(live::patch(app, &edits)?))
        }
        "secret_set" => {
            let name = arg_str(args, "name")?;
            let value = arg_str(args, "value")?;
            let value = value.trim();
            if value.is_empty() {
                return Err(msg("a secret can't be empty"));
            }
            if !known_secret(app, &name) {
                return Err(msg(format!("no source or allowed AI provider uses a secret named '{name}'")));
            }
            app.platform.secret_set(&name, value).map_err(|e| msg(format!("couldn't store {name}: {e}")))?;
            let names = live::restart_sources_using(app, &name);
            let message = if names.is_empty() { "Saved".to_string() } else { format!("Saved · {} reconnected", names.join(", ")) };
            let mut st = app.settings_state();
            st.rev += 1;
            st.message = message;
            Ok(json!(*st))
        }
        "fonts" => Ok(json!(fonts())),
        "slack_channels" => slack_channels(app, arg_i64(args, "source")?),
        "open_file" => {
            let path = app.settings_path();
            app.platform.open_url(&path.to_string_lossy()).map_err(|e| msg(format!("couldn't open {}: {e}", path.display())))?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

fn known_secret(app: &App, name: &str) -> bool {
    let settings = app.settings();
    settings.sources.iter().any(|s| sources::secrets(&s.kind, &s.config).iter().any(|k| k.name == name))
        || ai_secrets(&settings.ai).any(|(_, _, k, _)| *k == name)
}

fn ai_secrets(ai: &crate::settings::AiSettings) -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str, &'static str)> + '_ {
    crate::ai::PROVIDER_SECRETS.iter().filter(|(p, ..)| ai.allowed_providers.iter().any(|a| a == p))
}

fn get(app: &App) -> Result<Value> {
    // What's in effect (defaults filled in), but sources as written in the file:
    // the in-memory ones have the vault path and Life links resolved into them.
    let file: Value = std::fs::read_to_string(app.settings_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let mut values = serde_json::to_value(&*app.settings()).unwrap_or(Value::Null);
    if let Some(s) = file.get("sources") {
        values["sources"] = s.clone();
    }
    let status = crate::api::call(app, "sync.status", &json!({}))?;
    let glean_used = crate::glean::used_today(app).unwrap_or(0);
    let embed = crate::embed::call(app, "status", &json!({}))?;
    let settings = app.settings();
    let mut srcs = vec![];
    for (i, s) in settings.sources.iter().enumerate() {
        let row = status.as_array().and_then(|a| a.iter().find(|r| r["kind"] == s.kind.as_str() && r["name"] == s.name.as_str())).cloned();
        let secrets: Vec<Value> = sources::secrets(&s.kind, &s.config)
            .into_iter()
            .map(|k| {
                let set = app.platform.secret_get(&k.name).is_some();
                json!({ "name": k.name, "label": k.label, "note": k.note, "optional": k.optional, "oauth": k.oauth, "set": set, "set_at": app.platform.secret_set_at(&k.name) })
            })
            .collect();
        let missing = secrets.iter().any(|k| k["set"] == false && k["optional"] == false);
        let error = row.as_ref().and_then(|r| r["last_error"].as_str().map(str::to_owned));
        srcs.push(json!({
            "index": i,
            "kind": s.kind,
            "name": s.name,
            "tag": s.tag,
            "enabled": s.enabled,
            "status": row,
            "secrets": secrets,
            // The red dot: a required secret missing, or the last sync failed.
            "attention": s.enabled && (missing || error.is_some()),
            "used_today": if s.kind == "glean" { json!(glean_used) } else { Value::Null },
        }));
    }
    let (day_start, day_end) = app.clock.day_range(app.clock.today());
    let usage: Vec<Value> = app
        .store()
        .read(|r| r.usage().ai_by_model(day_start, day_end))?
        .into_iter()
        .map(|(model, calls, tokens)| json!({ "model": model, "calls": calls, "tokens": tokens }))
        .collect();
    let kinds: Vec<Value> = sources::KINDS
        .iter()
        .map(|(k, label)| json!({ "kind": k, "label": label, "fields": schema::source_fields(k), "defaults": sources::defaults(k) }))
        .collect();
    let ai_secrets: Vec<Value> = ai_secrets(&settings.ai)
        .map(|(provider, group, name, note)| {
            json!({ "provider": provider, "group": group, "name": name, "label": name, "note": note, "optional": false, "oauth": false,
                    "set": app.platform.secret_get(name).is_some(), "set_at": app.platform.secret_set_at(name) })
        })
        .collect();
    let tag_rules = crate::sync::PERSONAL_RULES;
    Ok(json!({
        "values": values,
        "fields": schema::settings_fields(),
        "kinds": kinds,
        "sources": srcs,
        "usage": usage,
        "ai_secrets": ai_secrets,
        "state": *app.settings_state(),
        "dev": cfg!(debug_assertions),
        "path": app.settings_path(),
        "tag_rules": tag_rules,
        "embed": embed,
    }))
}

/// Installed font families, sorted, with which are monospaced.
pub fn fonts() -> Vec<Value> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut fams: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for f in db.faces() {
        if let Some((name, _)) = f.families.first() {
            if name.starts_with('.') || name == "System Font" {
                continue;
            }
            let e = fams.entry(name.clone()).or_insert(true);
            *e = *e && f.monospaced;
        }
    }
    fams.into_iter().map(|(name, mono)| json!({ "name": name, "mono": mono })).collect()
}

/// Channels you're in, with what Alethiometer saw of them in the last 14 days.
fn slack_channels(app: &App, source_id: i64) -> Result<Value> {
    let source = app.store().read(|r| crate::sync::source(r, source_id))?;
    if source.kind != "slack" {
        return Err(msg("not a Slack source"));
    }
    let cfg = crate::sync::connectors::slack::Config::from_json(&source.config);
    let token = app.platform.secret_get(&cfg.user_token_secret).ok_or_else(|| msg(format!("set {} first", cfg.user_token_secret)))?;
    let http = crate::sync::http::UreqHttp::default();
    let chans = crate::sync::connectors::slack::member_channels(&http, token).map_err(|e| msg(e.to_string()))?;
    let since = app.clock.now() - jiff::SignedDuration::from_hours(24 * 14);
    let stats: std::collections::HashMap<String, (crate::time::Ts, i64)> =
        app.store().read(|r| r.sources().channel_activity(source_id, since))?.into_iter().map(|(c, last, n)| (c, (last, n))).collect();
    let out: Vec<Value> = chans
        .into_iter()
        .map(|(id, name, private)| {
            let (last, n) = stats.get(&id).map_or((None, 0), |(l, n)| (Some(*l), *n));
            json!({ "id": id, "name": name, "private": private, "last": last, "per_day": (n as f64 / 14.0 * 10.0).round() / 10.0 })
        })
        .collect();
    Ok(json!(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::context::tests::app;

    #[test]
    fn ai_provider_keys_are_settable_only_for_allowed_providers() {
        let (app, _dir) = app();
        assert!(call(&app, "secret_set", &json!({ "name": "digitalocean_api_key", "value": "k" })).is_err());
        app.update_settings(|s| s.ai.allowed_providers.push("digitalocean".into()));
        call(&app, "secret_set", &json!({ "name": "digitalocean_api_key", "value": "k" })).unwrap();
        let page = call(&app, "get", &json!({})).unwrap();
        let keys: Vec<_> = page["ai_secrets"].as_array().unwrap().iter().map(|k| (k["name"].as_str().unwrap(), k["set"].as_bool().unwrap())).collect();
        assert_eq!(keys, [("bedrock_api_key", false), ("digitalocean_api_key", true)]);
    }

    #[test]
    fn the_page_lists_sources_with_their_secrets_never_values() {
        let (app, _dir) = app();
        live::write_file(&app.settings_path(), &serde_json::to_value(&*app.settings()).unwrap()).unwrap();
        let src = json!({ "kind": "slack", "tag": "work", "name": "Slack", "config": { "channels": "all" } });
        live::patch(&app, &[Edit::parse(&json!({ "op": "push", "path": ["sources"], "value": src })).unwrap()]).unwrap();
        call(&app, "secret_set", &json!({ "name": "slack.user_token", "value": "xoxp-123" })).unwrap();
        let page = call(&app, "get", &json!({})).unwrap();
        let slack = &page["sources"][0];
        assert_eq!(slack["secrets"][0]["name"], "slack.user_token");
        assert_eq!(slack["secrets"][0]["set"], true);
        assert!(slack["secrets"][0]["set_at"].is_string());
        assert!(!page.to_string().contains("xoxp-123"), "values never leave");
        assert_eq!(slack["attention"], false, "the app token is optional");
        assert!(page["fields"].as_array().unwrap().iter().any(|f| f["key"] == "ui_scale"));
        assert!(call(&app, "secret_set", &json!({ "name": "someone.else", "value": "x" })).is_err(), "only secrets a source uses");
    }
}

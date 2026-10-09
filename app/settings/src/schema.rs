//! The Settings page's rows, generated from the JSON schema of `Settings`
//! and of each connector's config (`sources`). One row per field: label,
//! one-line description (the doc comment's first line), key path, control.

use super::Settings;
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Field {
    /// Path segments into settings.json (for a source: into `sources[i]`).
    pub path: Vec<Value>,
    /// `quiet_days`, `ai.region`, `config.lookback_hours`.
    pub key: String,
    pub section: String,
    pub group: String,
    pub label: String,
    pub desc: String,
    /// toggle, number, text, enum, list, map, tier, prices, font, folder,
    /// scale, hour, categories, person_color, code, masked_list,
    /// slack_channels, category, category_map, hour_range, glean_calls,
    /// swatches, segmented, palette.
    pub widget: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub opts: Value,
}

fn resolve<'a>(root: &'a Value, s: &'a Value) -> &'a Value {
    match s
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        Some(name) => root.get("$defs").and_then(|d| d.get(name)).unwrap_or(s),
        None => s,
    }
}

fn first_line(s: &Value) -> String {
    let t = s
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| s.get("description").and_then(Value::as_str))
        .unwrap_or("");
    t.lines().next().unwrap_or("").trim().to_string()
}

fn types(s: &Value) -> Vec<String> {
    match s.get("type") {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        _ => vec![],
    }
}

fn widget_for(root: &Value, s: &Value) -> String {
    if let Some(w) = s.get("x-widget").and_then(Value::as_str) {
        return w.into();
    }
    if s.get("x-enum").is_some() {
        return "enum".into();
    }
    let r = resolve(root, s);
    let t = types(r);
    if t.iter().any(|t| t == "boolean") {
        "toggle"
    } else if t.iter().any(|t| t == "integer" || t == "number") {
        "number"
    } else if t.iter().any(|t| t == "array") {
        "list"
    } else if t.iter().any(|t| t == "object") {
        "map"
    } else {
        "text"
    }
    .into()
}

const OPTS: &[&str] = &[
    "x-unit",
    "x-step",
    "x-min",
    "x-max",
    "x-enum",
    "x-readonly",
    "x-restart",
    "x-dev",
    "x-advanced",
    "x-secret",
    "x-note",
    "x-optional",
    "x-pair",
    "x-mono",
    "x-presets",
];

fn opts(s: &Value) -> Value {
    let mut m = Map::new();
    for k in OPTS {
        if let Some(v) = s.get(*k) {
            m.insert(k.trim_start_matches("x-").into(), v.clone());
        }
    }
    if m.is_empty() {
        Value::Null
    } else {
        Value::Object(m)
    }
}

fn label_of(name: &str, s: &Value) -> String {
    s.get("x-label")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let mut l = name.replace('_', " ");
            if let Some(c) = l.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            l
        })
}

/// Rows for an object schema's properties, under `prefix`.
fn walk(root: &Value, obj: &Value, prefix: &[Value], section: &str, out: &mut Vec<Field>) {
    let obj = resolve(root, obj);
    let Some(props) = obj.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, s) in props {
        if s.get("x-hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let mut path = prefix.to_vec();
        path.push(Value::String(name.clone()));
        let section = s
            .get("x-section")
            .and_then(Value::as_str)
            .unwrap_or(section)
            .to_string();
        let target = resolve(root, s);
        // A nested struct without its own widget: its fields are the rows.
        if s.get("x-widget").is_none() && target.get("properties").is_some() && s.get("x-label").is_none() {
            walk(root, target, &path, &section, out);
            continue;
        }
        let key = path_label(&path);
        out.push(Field {
            key,
            section: section.clone(),
            group: s.get("x-group").and_then(Value::as_str).unwrap_or("").into(),
            label: label_of(name, s),
            desc: first_line(s),
            widget: widget_for(root, s),
            opts: opts(s),
            path,
        });
    }
}

pub fn settings_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Settings)).unwrap_or(Value::Null)
}

/// Rows for General, Categories, People, AI and Developer.
pub fn settings_fields() -> Vec<Field> {
    let root = settings_schema();
    let mut out = vec![];
    walk(&root, &root, &[], "", &mut out);
    out.retain(|f| f.section != "sources");
    out
}

/// Rows for one source page, paths relative to `sources[i]`: name, tag and
/// poll interval, then the kind's config.
pub fn source_fields(kind: &str) -> Vec<Field> {
    let root = serde_json::to_value(schemars::schema_for!(super::SourceConfig)).unwrap_or(Value::Null);
    let mut out = vec![];
    walk(&root, &root, &[], "sources", &mut out);
    out.retain(|f| matches!(f.key.as_str(), "name" | "tag" | "poll_secs"));
    if let Some(cfg) = super::sources::schemas().get(kind) {
        walk(cfg, cfg, &[Value::String("config".into())], "sources", &mut out);
    }
    for f in &mut out {
        f.key = format!("sources[].{}", f.key);
    }
    out
}

/// Everything settings.json can hold, for editors (`settings.schema.json`).
pub fn document() -> Value {
    let mut root = settings_schema();
    root["x-source-configs"] = Value::Object(super::sources::schemas());
    root
}

/// `a.b[2].c`, for messages and the focused row's key path.
pub fn path_label(path: &[Value]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            Value::Number(n) => out.push_str(&format!("[{n}]")),
            Value::String(s) if s.contains('.') => out.push_str(&format!("[\"{s}\"]")),
            Value::String(s) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(s);
            }
            other => out.push_str(&other.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests;

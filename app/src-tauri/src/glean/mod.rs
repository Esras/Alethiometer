//! Glean: search-only. Queried on demand by ⌘K, the Search view and the
//! Assistant; nothing it returns is stored as an item or put in the local
//! index (results live in an in-memory cache for `cache_minutes`). Pins and
//! rejects per topic are the only thing kept (`search_feedback`).
//!
//! API (Glean Client API): `POST {server_url}/rest/api/v1/search` with a
//! bearer token (a Glean API token; `X-Glean-ActAs` for a global token).
//! Request and response shapes follow Glean's published OpenAPI spec
//! (`SearchRequest`, `SearchResponse` → `results[].document.metadata`).
//!
//! What's sent: the query text only. Queries are built from work context
//! (topics, work items, People notes); nothing from personal sources.

pub mod ask;
pub mod hydrate;

use crate::error::{msg, Result};
use crate::settings::sources::GleanConfig;
use crate::state::App;
use crate::sync::http::Http;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Who {
    pub name: String,
    pub email: Option<String>,
}

/// One Glean result, as much as the app uses.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Hit {
    pub title: String,
    pub url: String,
    /// Glean's datasource name (jira, confluence, gdrive, slack, o365sharepoint…).
    pub datasource: String,
    pub doc_type: String,
    pub mime: String,
    /// Where it lives ("PLAT", "Billing", "#billing-migration").
    pub container: String,
    pub author: Option<Who>,
    pub owner: Option<Who>,
    pub assignee: Option<Who>,
    pub updated_by: Option<Who>,
    pub mentioned: Vec<Who>,
    /// Local time "YYYY-MM-DDTHH:MM:SS".
    pub updated: Option<String>,
    pub snippet: String,
}

impl Hit {
    /// Everyone the result names, for entity matching and "who owns this".
    pub fn people(&self) -> impl Iterator<Item = &Who> {
        self.author.iter().chain(&self.owner).chain(&self.assignee).chain(&self.updated_by).chain(&self.mentioned)
    }
}

/// Why Glean can't be asked right now. Shown on the route in the facet bar.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Unavailable {
    /// No enabled glean source.
    Off,
    NoServer,
    NoToken { secret: String },
    Limit { per_day: i64 },
    Failed { error: String },
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unavailable::Off => write!(f, "Glean isn't set up (Settings → Sources → Add source)"),
            Unavailable::NoServer => write!(f, "Glean has no server URL (Settings → Sources → Glean)"),
            Unavailable::NoToken { secret } => write!(f, "Glean needs its API token ({secret})"),
            Unavailable::Limit { per_day } => write!(f, "Glean's daily limit is used up ({per_day} calls)"),
            Unavailable::Failed { error } => write!(f, "Glean: {error}"),
        }
    }
}

/// The enabled glean source's config, if any.
pub fn config(app: &App) -> Option<GleanConfig> {
    let s = app.settings();
    let src = s.sources.iter().find(|s| s.kind == "glean" && s.enabled && s.tag == "work")?;
    GleanConfig::from_json(&src.config).ok()
}

static CACHE: Mutex<Option<HashMap<String, (Instant, Vec<Hit>)>>> = Mutex::new(None);

/// The real date: the daily limit is Glean's quota, not the dev clock's.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn used_today(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT n FROM glean_calls WHERE day = ?1", [today()], |r| r.get(0)).optional()?.unwrap_or(0))
}

/// Search Glean for `query`. Cached per query for `cache_minutes`; counts
/// against `calls_per_day`.
pub fn search(app: &App, http: &dyn Http, query: &str) -> std::result::Result<Vec<Hit>, Unavailable> {
    let cfg = config(app).ok_or(Unavailable::Off)?;
    if cfg.server_url.trim().is_empty() {
        return Err(Unavailable::NoServer);
    }
    let token = app.platform.secret_get(&cfg.token_secret).ok_or(Unavailable::NoToken { secret: cfg.token_secret.clone() })?;
    let key = format!("{}\u{1}{}\u{1}{}\u{1}{}", app.data_dir.display(), query.trim(), cfg.datasources.join(","), cfg.exclude_datasources.join(","));
    if cfg.cache_minutes > 0 {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, hits)) = cache.as_ref().and_then(|c| c.get(&key)) {
            if at.elapsed() < Duration::from_secs(cfg.cache_minutes as u64 * 60) {
                return Ok(hits.clone());
            }
        }
    }
    {
        let conn = app.db();
        let used = used_today(&conn).map_err(|e| Unavailable::Failed { error: e.to_string() })?;
        if used >= cfg.calls_per_day {
            return Err(Unavailable::Limit { per_day: cfg.calls_per_day });
        }
        let _ = conn.execute(
            "INSERT INTO glean_calls (day, n) VALUES (?1, 1) ON CONFLICT (day) DO UPDATE SET n = n + 1",
            [today()],
        );
    }
    let body = request_body(&cfg, query);
    let url = format!("{}/rest/api/v1/search", cfg.server_url.trim_end_matches('/'));
    let auth = format!("Bearer {token}");
    let mut headers = vec![("authorization", auth.as_str())];
    if !cfg.act_as.is_empty() {
        headers.push(("x-glean-actas", cfg.act_as.as_str()));
    }
    let resp = http.post_json(&url, &headers, &body).map_err(|e| Unavailable::Failed { error: e.to_string() })?;
    let resp = match resp.status {
        401 | 403 => return Err(Unavailable::Failed { error: format!("Glean refused the token (HTTP {}); replace {} in Settings", resp.status, cfg.token_secret) }),
        429 => return Err(Unavailable::Failed { error: "Glean is rate limiting; try again in a minute".into() }),
        _ => resp.ok().map_err(|e| Unavailable::Failed { error: e.to_string() })?,
    };
    let v = resp.json().map_err(|e| Unavailable::Failed { error: format!("unreadable reply: {e}") })?;
    let hits = parse(&v);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.get_or_insert_with(HashMap::new).insert(key, (Instant::now(), hits.clone()));
    Ok(hits)
}

/// The SearchRequest. Excluded datasources are separate negated facet filters
/// (Glean ANDs different filters, ORs values within one).
pub fn request_body(cfg: &GleanConfig, query: &str) -> Value {
    let mut opts = json!({ "facetBucketSize": 10 });
    if !cfg.datasources.is_empty() {
        opts["datasourcesFilter"] = json!(cfg.datasources);
    }
    if !cfg.exclude_datasources.is_empty() {
        opts["facetFilters"] = Value::Array(
            cfg.exclude_datasources
                .iter()
                .map(|d| json!({ "fieldName": "datasource", "values": [{ "value": d, "relationType": "EQUALS", "isNegated": true }] }))
                .collect(),
        );
    }
    json!({
        "query": query,
        "pageSize": cfg.results_per_query.clamp(1, 100),
        "requestOptions": opts,
        "timeoutMillis": cfg.timeout_secs.clamp(1, 60) * 1000,
    })
}

fn who(v: Option<&Value>) -> Option<Who> {
    let v = v?;
    let name = v.get("name").and_then(Value::as_str)?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let email = v.get("metadata").and_then(|m| m.get("email")).and_then(Value::as_str).map(str::to_lowercase);
    Some(Who { name, email })
}

/// SearchResponse → hits. Unknown or missing fields are tolerated.
pub fn parse(v: &Value) -> Vec<Hit> {
    let results = v.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    results
        .iter()
        .filter_map(|r| {
            let doc = r.get("document").unwrap_or(&Value::Null);
            let meta = doc.get("metadata").unwrap_or(&Value::Null);
            let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let url = Some(s(r, "url")).filter(|u| !u.is_empty()).unwrap_or_else(|| s(doc, "url"));
            if url.is_empty() {
                return None;
            }
            let title = Some(s(r, "title")).filter(|t| !t.is_empty()).unwrap_or_else(|| s(doc, "title"));
            let snippet = r
                .get("snippets")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.get("snippet").or_else(|| x.get("text")).and_then(Value::as_str)).collect::<Vec<_>>().join(" … "))
                .unwrap_or_default();
            let updated = meta.get("updateTime").or_else(|| meta.get("createTime")).and_then(Value::as_str).and_then(crate::time::Ts::parse).map(crate::time::Ts::to_db);
            Some(Hit {
                title: if title.is_empty() { url.clone() } else { title },
                url,
                datasource: Some(s(doc, "datasource")).filter(|d| !d.is_empty()).unwrap_or_else(|| s(meta, "datasource")).to_lowercase(),
                doc_type: s(doc, "docType"),
                mime: s(meta, "mimeType"),
                container: s(meta, "container"),
                author: who(meta.get("author")),
                owner: who(meta.get("owner")),
                assignee: who(meta.get("assignedTo")),
                updated_by: who(meta.get("updatedBy")),
                mentioned: meta.get("mentionedPeople").and_then(Value::as_array).map(|a| a.iter().filter_map(|p| who(Some(p))).collect()).unwrap_or_default(),
                updated,
                snippet: snippet.chars().take(400).collect(),
            })
        })
        .collect()
}

/// The icon (in public/icons) and system name for a datasource.
pub fn system(datasource: &str, mime: &str, doc_type: &str) -> (&'static str, &'static str) {
    let office = |m: &str, d: &str| {
        let m = format!("{m} {d}").to_lowercase();
        // Spreadsheet and presentation MIME types contain "officedocument" too: check them first.
        if m.contains("sheet") || m.contains("excel") {
            Some(("excel", "Excel"))
        } else if m.contains("presentation") || m.contains("powerpoint") {
            Some(("powerpoint", "PowerPoint"))
        } else if m.contains("word") {
            Some(("word", "Word"))
        } else {
            None
        }
    };
    match datasource {
        "jira" | "jiradatacenter" => ("jira", "Jira"),
        "confluence" | "confluencedatacenter" => ("confluence", "Confluence"),
        "slack" | "slackentgrid" => ("slack", "Slack"),
        "github" | "githubenterprise" => ("github", "GitHub"),
        "gmail" => ("gmail", "Gmail"),
        "googlecalendar" | "gcalendar" => ("gcal", "Calendar"),
        "gdrive" | "googledrive" => ("gdrive", "Drive"),
        "microsoftteams" | "teams" => ("teams", "Teams"),
        "outlook" | "o365mail" | "exchange" => ("outlook", "Outlook"),
        "o365sharepoint" | "sharepoint" | "onedrive" | "o365onedrive" => office(mime, doc_type).unwrap_or(("onedrive", "OneDrive")),
        _ => office(mime, doc_type).unwrap_or(("glean", "Glean")),
    }
}

/// Save a pin or reject for a result in a topic (or clear it).
pub fn feedback(conn: &Connection, topic_id: i64, key: &str, verdict: &str, title: &str, url: &str, source: &str) -> Result<()> {
    match verdict {
        "pin" | "reject" => {
            conn.execute(
                "INSERT INTO search_feedback (topic_id, key, verdict, title, url, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (topic_id, key) DO UPDATE SET verdict = excluded.verdict, at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![topic_id, key, verdict, title, url, source],
            )?;
        }
        "clear" => {
            conn.execute("DELETE FROM search_feedback WHERE topic_id = ?1 AND key = ?2", params![topic_id, key])?;
        }
        other => return Err(msg(format!("unknown verdict '{other}' (pin, reject or clear)"))),
    }
    Ok(())
}

/// A topic's pins and rejects: key → (verdict, when).
pub fn feedback_for(conn: &Connection, topic_id: i64) -> Result<HashMap<String, (String, String)>> {
    let mut stmt = conn.prepare("SELECT key, verdict, at FROM search_feedback WHERE topic_id = ?1")?;
    let rows = stmt.query_map([topic_id], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, String>(2)?))))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ai::context::tests::app;
    use crate::sync::http::FakeHttp;

    pub const RESULTS: &str = include_str!("../../tests/fixtures/glean/search_renewal.json");

    pub fn with_glean(app: &App) {
        app.update_settings(|s| {
            s.sources.push(crate::settings::SourceConfig {
                kind: "glean".into(),
                tag: "work".into(),
                name: "Glean".into(),
                enabled: true,
                poll_secs: 300,
                config: json!({ "server_url": "https://acme-be.glean.com", "calls_per_day": 3 }),
            })
        });
        app.platform.secret_set("glean.token", "glean-tok").unwrap();
    }

    #[test]
    fn parses_the_published_response_shape() {
        let hits = parse(&serde_json::from_str(RESULTS).unwrap());
        assert!(hits.len() >= 5);
        let form = hits.iter().find(|h| h.title == "Datadog order form v2").unwrap();
        assert_eq!(form.datasource, "gdrive");
        assert_eq!(form.owner.as_ref().unwrap().name, "Kim Lee");
        assert_eq!(form.owner.as_ref().unwrap().email.as_deref(), Some("kim.lee@acme.com"));
        assert!(form.snippet.contains("SSO"));
        assert!(form.updated.is_some());
        assert_eq!(system("o365sharepoint", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", ""), ("excel", "Excel"));
        assert_eq!(system("confluence", "", ""), ("confluence", "Confluence"));
    }

    #[test]
    fn search_sends_bearer_counts_calls_and_caches() {
        let (app, _dir) = app();
        with_glean(&app);
        let http = FakeHttp::default();
        http.on("POST", "/rest/api/v1/search", 200, RESULTS);
        let a = search(&app, &http, "datadog renewal").unwrap();
        let b = search(&app, &http, "datadog renewal").unwrap();
        assert_eq!(a, b);
        assert_eq!(http.requests().len(), 1, "second call from the cache");
        let headers = http.header_log.lock().unwrap()[0].clone();
        assert!(headers.iter().any(|(k, v)| k == "authorization" && v == "Bearer glean-tok"));
        let body: Value = serde_json::from_str(http.requests()[0].2.as_deref().unwrap()).unwrap();
        assert_eq!(body["query"], "datadog renewal");
        assert_eq!(body["pageSize"], 30);
        assert_eq!(used_today(&app.db()).unwrap(), 1);
        search(&app, &http, "q2").unwrap();
        search(&app, &http, "q3").unwrap();
        assert_eq!(search(&app, &http, "q4").unwrap_err(), Unavailable::Limit { per_day: 3 });
    }

    #[test]
    fn unavailable_reasons() {
        let (app, _dir) = app();
        let http = FakeHttp::default();
        assert_eq!(search(&app, &http, "x").unwrap_err(), Unavailable::Off);
        with_glean(&app);
        http.on("POST", "/search", 401, "{}");
        assert!(matches!(search(&app, &http, "x").unwrap_err(), Unavailable::Failed { .. }));
        let cfg = GleanConfig { exclude_datasources: vec!["slack".into(), "hr".into()], ..Default::default() };
        let body = request_body(&cfg, "x");
        assert_eq!(body["requestOptions"]["facetFilters"].as_array().unwrap().len(), 2, "one negated filter each, so they AND");
    }
}

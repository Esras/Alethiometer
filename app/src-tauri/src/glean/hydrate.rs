//! Hydrated search: local results (FTS over work items) plus another route
//! (Glean today; any aggregator later), merged into one list. ⌘K (3b/3c) and
//! the Search view (18d) both use it.
//!
//! - A Glean result with the same URL as a local item merges into that item:
//!   one row, both routes, opens locally.
//! - Anchored to a topic, results that aren't about it are hidden behind a
//!   count ("9 off-topic hidden"). A local item is on topic when it's filed
//!   under the topic or its vector is close to the topic's. A Glean result
//!   needs both: an entity it shares with the topic (a person, Jira key,
//!   channel, link or repo) and similarity above `min_similarity` (shared
//!   words alone never keep it). Pins always keep a result; rejects drop it.
//!   Glean results older than `window_days` are dropped too.
//! - Unanchored, nothing is dropped; results rank by match and recency.

use super::{Hit, Unavailable};
use crate::error::Result;
use crate::state::App;
use crate::sync::http::Http;
use chrono::NaiveDateTime;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// Stable per result: `item:<id>` for local items, else the URL.
    pub key: String,
    pub title: String,
    pub url: Option<String>,
    /// Icon name in public/icons, and the system's name ("Jira", "Drive").
    pub icon: String,
    pub system: String,
    /// "glean" and/or "local".
    pub routes: Vec<&'static str>,
    pub at: Option<String>,
    pub when: String,
    pub snippet: String,
    pub item_id: Option<i64>,
    /// The item's topic (confirmed, else suggested), for local items.
    pub topic: Option<Value>,
    pub kept: bool,
    /// Why it's there or why it was dropped ("in topic", "Kim · .74", "shared words only").
    pub reason: String,
    pub similarity: Option<f32>,
    pub pinned: bool,
    /// Where it lives ("PROC", "#finops").
    pub container: String,
    /// Top-level messages (local items; the split review needs two).
    pub msgs: i64,
    /// People the result names (for "who owns this").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<super::Who>,
    #[serde(skip)]
    score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Facet {
    pub route: &'static str,
    pub count: usize,
    /// Sources this route returned, kept rows only: (icon, system, count).
    pub sources: Vec<Value>,
    /// Glean: why it wasn't asked or didn't answer; "not asked" until ⌥↵.
    pub status: Value,
}

pub struct Query<'a> {
    pub q: &'a str,
    pub anchor: Option<i64>,
    pub glean: bool,
    pub limit: usize,
}

/// What a topic is known by: the local check's entities.
#[derive(Debug, Default)]
pub struct Entities {
    /// Lowercased names (full and first) and emails of the topic's people.
    pub people: Vec<(String, String)>,
    pub keys: HashSet<String>,
    pub channels: HashSet<String>,
    pub urls: HashSet<String>,
    pub repos: HashSet<String>,
}

fn jira_keys(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\b[A-Z][A-Z0-9]{1,9}-\d+\b").expect("regex")).find_iter(text).map(|m| m.as_str().to_string()).collect()
}

fn norm_url(u: &str) -> String {
    u.trim().trim_end_matches('/').split('#').next().unwrap_or("").to_lowercase()
}

pub fn entities(app: &App, conn: &Connection, topic_id: i64) -> Result<Entities> {
    let mut e = Entities::default();
    let r = crate::queries::people::Resolver::load(conn, &app.settings().people)?;
    for p in crate::queries::people::for_topic(conn, &r, topic_id, app.clock.now(), 30)? {
        let name = p.name.trim().to_lowercase();
        let email = p
            .id
            .and_then(|id| conn.query_row("SELECT account FROM person_accounts WHERE person_id = ?1 AND kind = 'email' LIMIT 1", [id], |r| r.get::<_, String>(0)).ok())
            .unwrap_or_default();
        if name.len() >= 3 {
            e.people.push((name, email.to_lowercase()));
        }
    }
    let mut stmt = conn.prepare(
        "SELECT i.title, coalesce(i.url, ''), coalesce(json_extract(i.raw_json, '$.channel_name'), '')
           FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
          WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'",
    )?;
    for row in stmt.query_map([topic_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (title, url, channel) = row?;
        e.keys.extend(jira_keys(&title));
        if let Some(c) = title.strip_prefix('#').and_then(|t| t.split(|ch: char| ch.is_whitespace() || ch == '·').next()) {
            e.channels.insert(c.to_lowercase());
        }
        if !channel.is_empty() {
            e.channels.insert(channel.trim_start_matches('#').to_lowercase());
        }
        if !url.is_empty() {
            e.urls.insert(norm_url(&url));
        }
    }
    let (name, note): (String, Option<String>) = conn.query_row("SELECT name, obsidian_path FROM topics WHERE id = ?1", [topic_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    e.keys.extend(jira_keys(&name));
    if let Some(text) = note.and_then(|p| std::fs::read_to_string(app.vault.join(p)).ok()) {
        e.keys.extend(jira_keys(&text));
    }
    for repo in conn.prepare("SELECT repo FROM topic_repos WHERE topic_id = ?1")?.query_map([topic_id], |r| r.get::<_, String>(0))? {
        e.repos.insert(repo?.to_lowercase());
    }
    Ok(e)
}

/// The first entity `hit` shares with the topic, as a short label.
pub fn shared_entity(e: &Entities, hit: &Hit) -> Option<String> {
    for who in hit.people() {
        let n = who.name.to_lowercase();
        let email = who.email.clone().unwrap_or_default();
        for (pn, pe) in &e.people {
            let first = n.split_whitespace().next().unwrap_or("");
            if (!pe.is_empty() && *pe == email) || n == *pn || (!pn.contains(' ') && first == pn) {
                return Some(who.name.split_whitespace().next().unwrap_or(&who.name).to_string());
            }
        }
    }
    let text = format!("{} {} {}", hit.title, hit.snippet, hit.url);
    if let Some(k) = jira_keys(&text).into_iter().find(|k| e.keys.contains(k)) {
        return Some(k);
    }
    let container = hit.container.trim_start_matches('#').to_lowercase();
    if e.channels.contains(&container) {
        return Some(format!("#{container}"));
    }
    if e.urls.contains(&norm_url(&hit.url)) {
        return Some("same link".into());
    }
    let url = hit.url.to_lowercase();
    if let Some(r) = e.repos.iter().find(|r| url.contains(r.as_str())) {
        return Some(r.clone());
    }
    None
}

fn recency(at: Option<&str>, now: NaiveDateTime) -> f32 {
    let Some(t) = at.and_then(crate::queries::parse_ts) else { return 0.0 };
    let days = (now - t).num_hours().max(0) as f32 / 24.0;
    (-days / 30.0).exp()
}

fn when(at: Option<&str>, now: NaiveDateTime) -> String {
    let Some(t) = at.and_then(crate::queries::parse_ts) else { return String::new() };
    let days = (now - t).num_days();
    match days {
        d if d < 1 => format!("{}h", (now - t).num_hours().max(1)),
        d if d < 7 => format!("{d}d"),
        d if d < 30 => format!("{}w", d / 7),
        d if d < 365 => format!("{}mo", d / 30),
        d => format!("{}y", d / 365),
    }
}

fn source_label(kind: &str) -> &'static str {
    match kind {
        "slack" => "Slack",
        "gmail" => "Gmail",
        "gcal" => "Calendar",
        "jira" => "Jira",
        "github" => "GitHub",
        "obsidian" => "Obsidian",
        "notes" => "Notes",
        "sheets" => "Sheets",
        "notion" => "Notion",
        _ => "Local",
    }
}

/// Local work items matching `q`, best match first.
type LocalHit = (i64, String, Option<String>, String, String, Option<String>, String, i64);

/// Snippets mark matches with `search::HIT_START`/`HIT_END`, as in 18d.
fn local(conn: &Connection, q: &str, limit: usize) -> Result<Vec<LocalHit>> {
    let Some(fq) = crate::queries::search::fts_query(q) else { return Ok(vec![]) };
    let mut stmt = conn.prepare(
        "SELECT i.id, i.title, i.url, s.kind, coalesce(i.ended_at, i.occurred_at), coalesce(i.summary, ''),
                snippet(items_fts, 1, char(1), char(2), '…', 14),
                (SELECT count(*) FROM item_messages m WHERE m.item_id = i.id AND m.context = 0 AND m.reply_to IS NULL)
           FROM items_fts f JOIN items i ON i.id = f.rowid JOIN sources s ON s.id = i.source_id
          WHERE items_fts MATCH ?1 AND s.tag = 'work'
          ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![fq, limit as i64], |r| {
        let summary: String = r.get(5)?;
        let snip: String = r.get(6)?;
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, Some(summary).filter(|s| !s.is_empty()), snip, r.get(7)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn topic_of(conn: &Connection, item: i64) -> Option<Value> {
    conn.query_row(
        "SELECT t.id, t.name, t.category, tl.state FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
          WHERE tl.item_id = ?1 AND tl.state != 'wrong' AND t.archived_at IS NULL
          ORDER BY tl.state = 'confirmed' DESC, tl.updated_at DESC LIMIT 1",
        [item],
        |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "category": r.get::<_, String>(2)?, "state": r.get::<_, String>(3)? })),
    )
    .ok()
}

/// Run a hydrated search. `glean: false` gives the local route only (⌘K
/// before ⌥↵); the Glean route then says it wasn't asked.
pub fn run(app: &App, http: &dyn Http, query: &Query) -> Result<Value> {
    let now = app.clock.now();
    let cfg = super::config(app);
    let threshold = cfg.as_ref().map(|c| c.min_similarity as f32).unwrap_or(0.62);
    let window = cfg.as_ref().map(|c| c.window_days).unwrap_or(180);

    // Local route.
    let mut rows: Vec<Row> = vec![];
    let mut by_url: HashMap<String, usize> = HashMap::new();
    {
        let conn = app.db();
        for (rank, (id, title, url, kind, at, summary, snip, msgs)) in local(&conn, query.q, query.limit)?.into_iter().enumerate() {
            let topic = topic_of(&conn, id);
            if let Some(u) = &url {
                by_url.insert(norm_url(u), rows.len());
            }
            rows.push(Row {
                key: format!("item:{id}"),
                title,
                url,
                icon: kind.clone(),
                system: source_label(&kind).into(),
                routes: vec!["local"],
                when: when(Some(&at), now),
                at: Some(at),
                snippet: if snip.contains(crate::queries::search::HIT_START) || summary.is_none() { snip } else { summary.unwrap_or_default() },
                item_id: Some(id),
                msgs,
                topic,
                kept: true,
                reason: String::new(),
                similarity: None,
                pinned: false,
                container: String::new(),
                people: vec![],
                score: 1.0 / (1.0 + rank as f32),
            });
        }
    }

    // Glean route.
    let mut glean_status = json!({ "state": "not_asked" });
    let mut glean_hits: Vec<Hit> = vec![];
    if query.glean {
        match super::search(app, http, query.q) {
            Ok(h) => {
                glean_status = json!({ "state": "ok" });
                glean_hits = h;
            }
            Err(u) => glean_status = json!({ "state": state_name(&u), "message": u.to_string() }),
        }
    } else if cfg.is_none() {
        glean_status = json!({ "state": "off", "message": Unavailable::Off.to_string() });
    }
    let glean_start = rows.len();
    for (rank, h) in glean_hits.iter().enumerate() {
        if let Some(&i) = by_url.get(&norm_url(&h.url)) {
            // Same document locally: merge, keep the local row.
            rows[i].routes.push("glean");
            rows[i].people = h.people().cloned().collect();
            continue;
        }
        let (icon, system) = super::system(&h.datasource, &h.mime, &h.doc_type);
        rows.push(Row {
            key: h.url.clone(),
            title: h.title.clone(),
            url: Some(h.url.clone()),
            icon: icon.into(),
            system: system.into(),
            routes: vec!["glean"],
            when: when(h.updated.as_deref(), now),
            at: h.updated.clone(),
            snippet: h.snippet.clone(),
            item_id: None,
            msgs: 0,
            topic: None,
            kept: true,
            reason: String::new(),
            similarity: None,
            pinned: false,
            container: h.container.clone(),
            people: h.people().cloned().collect(),
            score: 1.0 / (1.0 + rank as f32),
        });
    }

    // Anchor: the local check.
    let mut anchor = Value::Null;
    if let Some(topic) = query.anchor {
        let (name, category): (String, String) = app.db().query_row("SELECT name, category FROM topics WHERE id = ?1", [topic], |r| Ok((r.get(0)?, r.get(1)?)))?;
        anchor = json!({ "id": topic, "name": name, "category": category });
        let (ents, feedback) = {
            let conn = app.db();
            (entities(app, &conn, topic)?, super::feedback_for(&conn, topic)?)
        };
        let tv = crate::embed::topic_vector(app, topic);
        // Similarity: stored vectors for local items, embedded now for Glean results.
        let texts: Vec<String> = rows[glean_start..].iter().map(|r| format!("{}\n{}\n{}", r.title, r.container, r.snippet)).collect();
        let glean_vecs = match (&tv, texts.is_empty()) {
            (Some(_), false) => crate::embed::embed_now(app, &texts),
            _ => None,
        };
        let hits_by_url: HashMap<String, &Hit> = glean_hits.iter().map(|h| (norm_url(&h.url), h)).collect();
        for (i, row) in rows.iter_mut().enumerate() {
            let sim = match (&tv, row.item_id) {
                (Some(t), Some(id)) => crate::embed::vector(&app.db(), "item", id).ok().flatten().map(|v| crate::embed::cosine(t, &v)),
                (Some(t), None) => glean_vecs.as_ref().and_then(|v| v.get(i - glean_start)).map(|v| crate::embed::cosine(t, v)),
                (None, _) => None,
            };
            row.similarity = sim;
            let fb = feedback.get(&row.key).or_else(|| row.url.as_ref().and_then(|u| feedback.get(u)));
            let in_topic = row.topic.as_ref().is_some_and(|t| t["id"] == topic);
            let sim_label = sim.map(|s| format!(".{:02}", (s.clamp(0.0, 0.99) * 100.0) as i32)).unwrap_or_default();
            let (kept, reason) = if let Some((v, at)) = fb.filter(|(v, _)| v == "reject") {
                let _ = v;
                (false, format!("rejected {}", short_date(at)))
            } else if fb.is_some() {
                row.pinned = true;
                (true, "pinned".into())
            } else if in_topic {
                (true, "in topic".into())
            } else if row.item_id.is_some() {
                match sim {
                    Some(s) if s >= threshold => (true, sim_label.clone()),
                    Some(_) => (false, "another topic".into()),
                    None => (false, "not in this topic".into()),
                }
            } else {
                let h = hits_by_url.get(&norm_url(row.url.as_deref().unwrap_or("")));
                let old = row.at.as_deref().and_then(crate::queries::parse_ts).is_some_and(|t| (now - t).num_days() > window);
                let shared = h.and_then(|h| shared_entity(&ents, h));
                match (old, shared, sim) {
                    (true, _, _) => (false, format!("outside {window}d")),
                    (false, None, _) => (false, "shared words only".into()),
                    (false, Some(e), Some(s)) if s >= threshold => (true, format!("{e} · {sim_label}")),
                    (false, Some(e), None) => (true, e),
                    (false, Some(_), Some(_)) => (false, format!("low similarity {sim_label}")),
                }
            };
            row.kept = kept;
            row.reason = reason;
            row.score = if row.pinned { 10.0 } else { 0.0 } + if in_topic { 0.3 } else { 0.0 } + sim.unwrap_or(0.5) + 0.2 * recency(row.at.as_deref(), now);
        }
    } else {
        for row in &mut rows {
            row.score = 0.6 * row.score + 0.4 * recency(row.at.as_deref(), now);
        }
    }
    rows.sort_by(|a, b| b.kept.cmp(&a.kept).then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)));

    let facet = |route: &'static str, status: Value| {
        let mut sources: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut count = 0;
        for r in rows.iter().filter(|r| r.kept && r.routes.contains(&route)) {
            count += 1;
            *sources.entry((r.icon.clone(), r.system.clone())).or_default() += 1;
        }
        let mut sources: Vec<((String, String), usize)> = sources.into_iter().collect();
        sources.sort_by(|a, b| b.1.cmp(&a.1));
        Facet { route, count, sources: sources.into_iter().map(|((icon, system), n)| json!({ "icon": icon, "system": system, "count": n })).collect(), status }
    };
    let facets = vec![facet("glean", glean_status), facet("local", json!({ "state": "ok" }))];
    let hidden = rows.iter().filter(|r| !r.kept).count();
    let topics: Vec<Value> = {
        let conn = app.db();
        let like = format!("%{}%", query.q.trim());
        let mut stmt = conn.prepare("SELECT id, name, category FROM topics WHERE archived_at IS NULL AND name LIKE ?1 ORDER BY name LIMIT 5")?;
        let rows = stmt.query_map([like], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "category": r.get::<_, String>(2)? })))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    Ok(json!({
        "q": query.q,
        "anchor": anchor,
        "rows": rows,
        "facets": facets,
        "hidden": hidden,
        "topics": topics,
        "embeddings": crate::embed::call(app, "status", &json!({}))?["status"]["state"],
    }))
}

fn short_date(at: &str) -> String {
    crate::queries::parse_ts(at).map(|t| t.format("%b %-d").to_string()).unwrap_or_default()
}

pub fn state_name(u: &Unavailable) -> &'static str {
    match u {
        Unavailable::Off => "off",
        Unavailable::NoServer => "no_server",
        Unavailable::NoToken { .. } => "no_token",
        Unavailable::Limit { .. } => "limit",
        Unavailable::Failed { .. } => "failed",
    }
}

/// `search.hydrated {q, anchor?, glean?}`, `search.feedback {topic_id, key, verdict, title?, url?, source?}`.
pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "hydrated" => {
            let q = crate::queries::arg_str(args, "q")?;
            let query = Query {
                q: &q,
                anchor: crate::queries::opt_i64(args, "anchor"),
                glean: args.get("glean").and_then(Value::as_bool).unwrap_or(false),
                limit: crate::queries::opt_i64(args, "limit").unwrap_or(40).clamp(5, 100) as usize,
            };
            let timeout = super::config(app).map(|c| c.timeout_secs.clamp(1, 60) as u64 + 2).unwrap_or(10);
            let http = crate::sync::http::UreqHttp::with_timeout(timeout);
            run(app, &http, &query)
        }
        "feedback" => {
            let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            super::feedback(&app.db(), crate::queries::arg_i64(args, "topic_id")?, &crate::queries::arg_str(args, "key")?, &crate::queries::arg_str(args, "verdict")?, &s("title"), &s("url"), &s("source"))?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::context::tests::{app, topic_id};
    use crate::glean::tests::{with_glean, RESULTS};
    use crate::sync::http::FakeHttp;
    use std::sync::Arc;

    fn http() -> FakeHttp {
        let h = FakeHttp::default();
        h.on("POST", "/rest/api/v1/search", 200, RESULTS);
        h
    }

    #[test]
    fn local_only_until_glean_is_asked() {
        let (app, _dir) = app();
        with_glean(&app);
        let h = http();
        let v = run(&app, &h, &Query { q: "renewal", anchor: None, glean: false, limit: 40 }).unwrap();
        assert!(h.requests().is_empty(), "no Glean call before ⌥↵");
        assert_eq!(v["facets"][0]["status"]["state"], "not_asked");
        assert!(v["rows"].as_array().unwrap().iter().all(|r| r["routes"] == json!(["local"])));
        assert!(v["facets"][1]["count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn unanchored_keeps_everything_and_merges_same_urls() {
        let (app, _dir) = app();
        with_glean(&app);
        let proc412: i64 = app.db().query_row("SELECT id FROM items WHERE title LIKE 'PROC-412%' LIMIT 1", [], |r| r.get(0)).unwrap();
        app.db().execute("UPDATE items SET url = 'https://jira.example.com/browse/PROC-412' WHERE id = ?1", [proc412]).unwrap();
        let v = run(&app, &http(), &Query { q: "renewal", anchor: None, glean: true, limit: 40 }).unwrap();
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(v["hidden"], 0);
        let merged = rows.iter().find(|r| r["item_id"] == proc412).unwrap();
        assert_eq!(merged["routes"], json!(["local", "glean"]), "one row, both routes");
        assert_eq!(rows.iter().filter(|r| r["url"] == "https://jira.example.com/browse/PROC-412").count(), 1);
        assert!(rows.iter().any(|r| r["title"] == "Renewal: Adobe Creative Cloud" && r["icon"] == "word"));
        let glean = &v["facets"][0];
        assert_eq!(glean["status"]["state"], "ok");
        assert!(glean["sources"].as_array().unwrap().iter().any(|s| s["icon"] == "gdrive"));
    }

    #[test]
    fn anchored_hides_off_topic_by_entity_and_similarity() {
        let (app, _dir) = app();
        with_glean(&app);
        app.embed.install(Arc::new(crate::embed::tests::Stub));
        while crate::embed::index_pass(&app, &crate::embed::tests::Stub, 100).unwrap() > 0 {}
        // The stub embedder's scores are low; judge by entities here.
        app.update_settings(|s| s.sources.last_mut().unwrap().config["min_similarity"] = json!(0.0));
        let dd = topic_id(&app, "Datadog renewal");
        let v = run(&app, &http(), &Query { q: "renewal", anchor: Some(dd), glean: true, limit: 40 }).unwrap();
        let rows = v["rows"].as_array().unwrap();
        let row = |t: &str| rows.iter().find(|r| r["title"] == t).unwrap_or_else(|| panic!("{t}: {rows:#?}"));
        assert_eq!(row("Datadog order form v2")["kept"], true, "Kim owns it");
        assert!(row("Datadog order form v2")["reason"].as_str().unwrap().starts_with("Kim"));
        assert_eq!(row("Finance sync · renewal notes")["kept"], true, "Ana and Kim");
        assert_eq!(row("Payroll vendor renewal sync")["kept"], false);
        assert_eq!(row("Payroll vendor renewal sync")["reason"], "shared words only");
        assert_eq!(row("Datadog cutover plan (2024)")["reason"], "outside 180d");
        assert!(v["hidden"].as_u64().unwrap() >= 3);
        // Kept rows first.
        let first_dropped = rows.iter().position(|r| r["kept"] == false).unwrap();
        assert!(rows[first_dropped..].iter().all(|r| r["kept"] == false));
        // Reject and pin, per topic.
        crate::glean::feedback(&app.db(), dd, "https://docs.google.com/document/d/dd-order-form-v2", "reject", "", "", "").unwrap();
        crate::glean::feedback(&app.db(), dd, "https://teams.microsoft.com/l/meeting/payroll-renewal", "pin", "", "", "").unwrap();
        let v = run(&app, &http(), &Query { q: "renewal", anchor: Some(dd), glean: true, limit: 40 }).unwrap();
        let rows = v["rows"].as_array().unwrap();
        let row = |t: &str| rows.iter().find(|r| r["title"] == t).unwrap();
        assert_eq!(row("Datadog order form v2")["kept"], false);
        assert!(row("Datadog order form v2")["reason"].as_str().unwrap().starts_with("rejected"));
        assert_eq!(rows[0]["title"], "Payroll vendor renewal sync", "pins come first");
    }

    #[test]
    fn without_a_glean_source_the_route_says_so() {
        let (app, _dir) = app();
        let v = run(&app, &http(), &Query { q: "renewal", anchor: None, glean: true, limit: 40 }).unwrap();
        assert_eq!(v["facets"][0]["status"]["state"], "off");
    }
}

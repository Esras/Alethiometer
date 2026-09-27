//! Notes (9a, then 10a/11a): a free-writing page per day. Each line has a kind
//! (from its prefix) and a "Goes to" destination: a topic, a person or an app.
//! `⌃↵` sends every work line that has a destination into Incoming as a new
//! signal; lines without one are held on the page.
//!
//! Destinations come from a plain word-matching heuristic (`suggest`); no AI.
//!
//! `~` life lines never go to Incoming or the AI. Sending them to Marvin (with a
//! receipt) is Phase 2, so here they only carry the receipt *states*
//! (pending / confirmed / failed) for the UI; nothing is ever deleted locally.
//!
//! `notes_lines.target_json` holds a [`Meta`]: the user's own destination
//! choice (if any, flattened as `{type, id?, label, …}`), whether the line has
//! no glyph (`bare`), and the life-line receipt fields. Suggested destinations
//! are not stored; they are recomputed on read so they follow the text.

use super::{arg_str, opt_i64, parse_date};
use crate::api::unknown;
use crate::error::{msg, Result};
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use chrono::{Duration, NaiveDate};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

const KINDS: &[&str] = &["note", "do", "research", "delegated", "life"];
/// States a line can still be edited (and sent) in.
const EDITABLE: &[&str] = &["draft", "held"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// topic | person | app | keep
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// For app targets: jira | marvin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Jira key for app targets ("PLAT-1201").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Topic behind an app/person target, used to file the Incoming signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<i64>,
    /// Quiet offer shown under the destination ("create Jira task"). Never acted on by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
    /// The user accepted the offer: on send, a create-issue write is queued in the outbox.
    #[serde(default, skip_serializing_if = "is_false")]
    pub create: bool,
    /// True for a suggestion; false for the user's own pick.
    #[serde(default)]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Target {
    /// Same destination, ignoring suggestion details.
    fn same(&self, o: &Target) -> bool {
        self.ty == o.ty && self.id == o.id && self.key == o.key && self.label == o.label
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// What `notes_lines.target_json` stores.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Meta {
    #[serde(flatten, default)]
    to: Option<Target>,
    #[serde(default, skip_serializing_if = "is_false")]
    bare: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receipt_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Meta {
    fn parse(s: Option<&str>) -> Meta {
        s.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
    }
}

#[derive(Debug, Serialize)]
struct Line {
    id: i64,
    /// Echo of the client's key for this line (save), so it can match rows up.
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    kind: String,
    bare: bool,
    text: String,
    /// Effective destination: the user's pick, else the suggestion.
    target: Option<Target>,
    state: String,
    receipt_at: Option<String>,
    error: Option<String>,
    locked: bool,
    /// The Jira task this line asked for, once queued: {state, key?, url?, error?}.
    jira: Option<Value>,
}

struct Row {
    id: i64,
    kind: String,
    text: String,
    state: String,
    meta: Meta,
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "page" => {
            let date = page_date(app, args)?;
            page(app, &app.db(), date, &HashMap::new())
        }
        "save" => save(app, args),
        "send" => {
            let date = page_date(app, args)?;
            send(app, date)?;
            page(app, &app.db(), date, &HashMap::new())
        }
        "suggest" => {
            let conn = app.db();
            let idx = Index::load(&conn, &app.vault)?;
            let text = arg_str(args, "text")?;
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("note");
            Ok(json!(idx.suggest(kind, &text)))
        }
        "options" => options(app, args),
        "retry" => {
            let date = page_date(app, args)?;
            retry(&app.db(), date, opt_i64(args, "id"))?;
            // ⌘R also retries this page's failed Jira task creations.
            app.db().execute(
                "UPDATE outbox SET state = 'pending' WHERE state = 'failed' AND op = 'jira.create_issue'
                   AND json_extract(payload_json, '$.notes_line_id') IN (SELECT id FROM notes_lines WHERE page_date = ?1)",
                [date.format("%Y-%m-%d").to_string()],
            )?;
            page(app, &app.db(), date, &HashMap::new())
        }
        _ => Err(unknown(name)),
    }
}

fn page_date(app: &App, args: &Value) -> Result<NaiveDate> {
    match args.get("date").and_then(Value::as_str) {
        Some(d) if !d.is_empty() => parse_date(d),
        _ => Ok(app.clock.now().date()),
    }
}

fn load_rows(conn: &Connection, date: NaiveDate) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, text, state, target_json FROM notes_lines WHERE page_date = ?1 ORDER BY position, id",
    )?;
    let rows = stmt
        .query_map([date.to_string()], |r| {
            let meta: Option<String> = r.get(4)?;
            Ok(Row { id: r.get(0)?, kind: r.get(1)?, text: r.get(2)?, state: r.get(3)?, meta: Meta::parse(meta.as_deref()) })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn marvin() -> Target {
    Target {
        ty: "app".into(),
        id: None,
        label: "Marvin · Inbox".into(),
        category: None,
        app: Some("marvin".into()),
        key: None,
        topic_id: None,
        offer: None,
        create: false,
        auto: true,
        reason: None,
    }
}

/// The destination shown for a line: stored choice, else a fresh suggestion.
fn effective(idx: &Index, r: &Row) -> Option<Target> {
    if r.kind == "life" {
        return Some(marvin());
    }
    if let Some(t) = &r.meta.to {
        return Some(t.clone());
    }
    if r.text.trim().is_empty() || !EDITABLE.contains(&r.state.as_str()) {
        return None;
    }
    idx.suggest(&r.kind, &r.text)
}

/// Latest `jira.create_issue` write queued for a line, with the new key once
/// Jira confirmed it.
fn jira_create(conn: &Connection, line_id: i64) -> Option<Value> {
    conn.query_row(
        "SELECT state, error, receipt_json FROM outbox
          WHERE op = 'jira.create_issue' AND json_extract(payload_json, '$.notes_line_id') = ?1
          ORDER BY id DESC LIMIT 1",
        [line_id],
        |r| {
            let receipt: Option<String> = r.get(2)?;
            let receipt: Value = receipt.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            Ok(json!({
                "state": r.get::<_, String>(0)?,
                "error": r.get::<_, Option<String>>(1)?,
                "key": receipt.get("key"),
                "url": receipt.get("url"),
            }))
        },
    )
    .ok()
}

fn page(app: &App, conn: &Connection, date: NaiveDate, keys: &HashMap<i64, String>) -> Result<Value> {
    let today = app.clock.now().date();
    let idx = Index::load(conn, &app.vault)?;
    let rows = load_rows(conn, date)?;
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| Line {
            id: r.id,
            key: keys.get(&r.id).cloned(),
            kind: r.kind.clone(),
            bare: r.meta.bare,
            text: r.text.clone(),
            target: effective(&idx, r),
            state: r.state.clone(),
            receipt_at: r.meta.receipt_at.clone(),
            error: r.meta.error.clone(),
            locked: !EDITABLE.contains(&r.state.as_str()),
            jira: jira_create(conn, r.id),
        })
        .collect();
    let label = match (date - today).num_days() {
        0 => "Today's page".to_string(),
        -1 => "Yesterday".to_string(),
        _ => date.format("%a, %b %-d").to_string(),
    };
    Ok(json!({
        "date": date.to_string(),
        "label": label,
        "today": date == today,
        "prev": (date - Duration::days(1)).to_string(),
        "next": if date < today { Some((date + Duration::days(1)).to_string()) } else { None },
        "lines": lines,
    }))
}

#[derive(Deserialize)]
struct SaveLine {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    id: Option<i64>,
    kind: String,
    #[serde(default)]
    bare: bool,
    text: String,
    /// Only the user's own pick; suggestions are recomputed.
    #[serde(default)]
    target: Option<Target>,
}

/// Autosave: the whole page, in order. Lines missing from the list are removed,
/// but only while still editable (draft/held); sent or receipt-tracked lines
/// are never deleted here.
fn save(app: &App, args: &Value) -> Result<Value> {
    let date = page_date(app, args)?;
    let lines: Vec<SaveLine> = serde_json::from_value(args.get("lines").cloned().unwrap_or(json!([])))?;
    let mut conn = app.db();
    let tx = conn.transaction()?;
    let existing: HashMap<i64, Row> = load_rows(&tx, date)?.into_iter().map(|r| (r.id, r)).collect();
    let mut keep: HashSet<i64> = HashSet::new();
    let mut keys = HashMap::new();
    for (pos, l) in lines.iter().enumerate() {
        if !KINDS.contains(&l.kind.as_str()) {
            return Err(msg(format!("bad kind '{}'", l.kind)));
        }
        let old = l.id.and_then(|id| existing.get(&id));
        let id = match old {
            Some(r) if !EDITABLE.contains(&r.state.as_str()) => {
                // Locked: only its position can change.
                tx.execute("UPDATE notes_lines SET position = ?1 WHERE id = ?2", params![pos as i64, r.id])?;
                r.id
            }
            Some(r) => {
                let mut meta = r.meta.clone();
                meta.bare = l.bare && l.kind == "note";
                meta.to = l.target.clone().filter(|t| !t.auto && l.kind != "life").map(|mut t| {
                    t.auto = false;
                    t
                });
                tx.execute(
                    "UPDATE notes_lines SET position = ?1, kind = ?2, text = ?3, target_json = ?4 WHERE id = ?5",
                    params![pos as i64, l.kind, l.text, serde_json::to_string(&meta)?, r.id],
                )?;
                r.id
            }
            None => {
                let meta = Meta {
                    to: l.target.clone().filter(|t| !t.auto && l.kind != "life"),
                    bare: l.bare && l.kind == "note",
                    ..Default::default()
                };
                tx.execute(
                    "INSERT INTO notes_lines (page_date, position, kind, text, target_json, state)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'draft')",
                    params![date.to_string(), pos as i64, l.kind, l.text, serde_json::to_string(&meta)?],
                )?;
                tx.last_insert_rowid()
            }
        };
        keep.insert(id);
        if let Some(k) = &l.key {
            keys.insert(id, k.clone());
        }
    }
    for r in existing.values() {
        if !keep.contains(&r.id) && EDITABLE.contains(&r.state.as_str()) {
            tx.execute("DELETE FROM notes_lines WHERE id = ?1", [r.id])?;
        }
    }
    let out = page(app, &tx, date, &keys)?;
    tx.commit()?;
    Ok(out)
}

/// Local work source that Notes signals come from.
fn helm_source(conn: &Connection) -> Result<i64> {
    let id: Option<i64> = conn.query_row("SELECT id FROM sources WHERE kind = 'helm'", [], |r| r.get(0)).optional()?;
    match id {
        Some(id) => Ok(id),
        None => {
            conn.execute("INSERT INTO sources (kind, tag, name) VALUES ('helm', 'work', 'Helm Notes')", [])?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Each editable work line with a destination becomes a new Incoming signal
/// (an unseen item). Lines with no destination are held. Life lines and
/// "keep as a note" lines stay where they are.
fn send(app: &App, date: NaiveDate) -> Result<()> {
    let now = crate::seed::fmt(app.clock.now());
    let mut conn = app.db();
    let tx = conn.transaction()?;
    let idx = Index::load(&tx, &app.vault)?;
    let rows = load_rows(&tx, date)?;
    let src = helm_source(&tx)?;
    for r in rows {
        if r.kind == "life" || !EDITABLE.contains(&r.state.as_str()) || r.text.trim().is_empty() {
            continue;
        }
        let Some(t) = effective(&idx, &r) else {
            tx.execute("UPDATE notes_lines SET state = 'held' WHERE id = ?1", [r.id])?;
            continue;
        };
        if t.ty == "keep" {
            continue;
        }
        let delegate = if r.kind == "delegated" { mention(&r.text) } else { None };
        let mut summary = match r.kind.as_str() {
            "do" => "To do".to_string(),
            "research" => "Look into".to_string(),
            "delegated" => format!("Delegated to {}", delegate.clone().unwrap_or_else(|| "someone".into())),
            _ => "Note".to_string(),
        };
        match t.ty.as_str() {
            "person" => summary.push_str(&format!(" · about {}", t.label.trim_start_matches("People/"))),
            "app" => summary.push_str(&format!(" · {}", t.label)),
            _ => {}
        }
        summary.push_str(" · from Notes");
        let owner = match r.kind.as_str() {
            "do" | "research" => Some("you".to_string()),
            "delegated" => delegate.clone(),
            _ => None,
        };
        let item_kind = if matches!(r.kind.as_str(), "do" | "delegated") { "task" } else { "note" };
        tx.execute(
            "INSERT INTO items (source_id, external_id, kind, title, summary, occurred_at, status, owner, seen, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, 0, ?8)",
            params![
                src,
                format!("notes-line-{}", r.id),
                item_kind,
                r.text.trim(),
                summary,
                now,
                owner,
                json!({ "notes_line_id": r.id, "page_date": date.to_string(), "kind": r.kind, "target": t }).to_string(),
            ],
        )?;
        let item = tx.last_insert_rowid();
        // Filed under its topic as a suggestion, so it lands in that topic's Incoming
        // list for confirm / wrong-topic. Person targets without a single home topic
        // stay unlinked and show up in Unsorted.
        let topic = match t.ty.as_str() {
            "topic" => t.id,
            _ => t.topic_id,
        };
        if let Some(topic) = topic {
            let reason = t.reason.clone().unwrap_or_else(|| if t.auto { "Notes suggestion".into() } else { "you picked it in Notes".into() });
            tx.execute(
                "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'suggested', ?3, ?4)",
                params![item, topic, if t.auto { 0.7 } else { 1.0 }, reason],
            )?;
        }
        if t.create {
            let jira: Option<i64> = tx.query_row("SELECT id FROM sources WHERE kind = 'jira'", [], |r| r.get(0)).optional()?;
            if let Some(jira) = jira {
                tx.execute(
                    "INSERT INTO outbox (source_id, op, payload_json, item_id) VALUES (?1, 'jira.create_issue', ?2, ?3)",
                    params![jira, json!({ "summary": r.text.trim(), "assignee": delegate, "notes_line_id": r.id, "item_id": item }).to_string(), item],
                )?;
            }
        }
        let mut meta = r.meta.clone();
        meta.to = Some(Target { auto: t.auto, ..t });
        tx.execute(
            "UPDATE notes_lines SET state = 'incoming', target_json = ?1 WHERE id = ?2",
            params![serde_json::to_string(&meta)?, r.id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// ⌘R: failed life lines go back to pending. They were never removed.
fn retry(conn: &Connection, date: NaiveDate, id: Option<i64>) -> Result<()> {
    let rows = load_rows(conn, date)?;
    for r in rows.into_iter().filter(|r| r.kind == "life" && r.state == "failed" && id.map_or(true, |i| i == r.id)) {
        let mut meta = r.meta;
        meta.error = None;
        conn.execute(
            "UPDATE notes_lines SET state = 'pending', target_json = ?1 WHERE id = ?2",
            params![serde_json::to_string(&meta)?, r.id],
        )?;
    }
    Ok(())
}

/// Picker contents: with no query, the next likely destinations after the
/// current one; with a query, topics / people / Jira keys matching it.
fn options(app: &App, args: &Value) -> Result<Value> {
    let conn = app.db();
    let idx = Index::load(&conn, &app.vault)?;
    let text = args.get("text").and_then(Value::as_str).unwrap_or("");
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("note");
    let q = args.get("q").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let mut out: Vec<Target> = vec![];
    if q.is_empty() {
        let best = idx.suggest(kind, text);
        let seen = |out: &Vec<Target>, t: &Target| best.as_ref().map_or(false, |b| b.same(t)) || out.iter().any(|o| o.same(t));
        for t in idx.ranked(kind, text) {
            if !seen(&out, &t) {
                out.push(t);
            }
        }
        // Pad with the busiest topics so there are always a few to pick from.
        for t in &idx.topics {
            if out.len() >= 3 {
                break;
            }
            let tt = idx.topic_target(t, None);
            if !seen(&out, &tt) {
                out.push(tt);
            }
        }
        out.truncate(3);
    } else {
        for t in idx.topics.iter().filter(|t| t.name.to_lowercase().contains(&q)) {
            out.push(idx.topic_target(t, None));
        }
        for p in idx.people.iter().filter(|p| p.name.to_lowercase().contains(&q)) {
            out.push(idx.person_target(p));
        }
        for i in idx.issues.iter().filter(|i| i.key.to_lowercase().contains(&q) || i.title.to_lowercase().contains(&q)) {
            out.push(idx.issue_target(i));
        }
        out.truncate(6);
    }
    for t in &mut out {
        t.auto = false;
        t.reason = None;
    }
    // A delegated line can take up its offer: create a Jira task for the delegate.
    let create = (kind == "delegated").then(|| {
        let who = mention(text).unwrap_or_else(|| "them".into());
        let base = idx.suggest(kind, text);
        Target {
            ty: "app".into(),
            id: None,
            label: format!("Jira · new task for {who}"),
            category: None,
            app: Some("jira".into()),
            key: None,
            topic_id: base.as_ref().and_then(|b| if b.ty == "topic" { b.id } else { b.topic_id }),
            offer: None,
            create: true,
            auto: false,
            reason: None,
        }
    });
    Ok(json!({ "options": out, "create": create }))
}

// ---------------------------------------------------------------------------
// Suggestion heuristic. Plain word matching over topic names, the topics' linked
// items and Obsidian notes, people names, Jira keys and #tags. No AI yet.

struct TopicDoc {
    id: i64,
    name: String,
    category: String,
    name_words: HashSet<String>,
    words: HashSet<String>,
    people: HashSet<i64>,
}

struct PersonDoc {
    id: i64,
    name: String,
    topics: Vec<i64>,
}

struct IssueDoc {
    key: String,
    title: String,
    words: HashSet<String>,
    topic_id: Option<i64>,
}

struct Index {
    topics: Vec<TopicDoc>,
    people: Vec<PersonDoc>,
    issues: Vec<IssueDoc>,
    names: HashSet<String>,
}

const STOP: &[&str] = &[
    "the", "and", "for", "but", "not", "with", "about", "from", "into", "that", "this", "then", "than", "them", "they",
    "their", "there", "what", "when", "where", "which", "while", "who", "whom", "why", "how", "are", "was", "were", "been",
    "being", "have", "has", "had", "does", "did", "doing", "can", "could", "should", "would", "will", "just", "only", "also",
    "very", "more", "most", "some", "any", "all", "each", "other", "others", "our", "your", "you", "yours", "its", "his",
    "her", "hers", "him", "she", "one", "two", "get", "got", "make", "makes", "made", "sense", "check", "whether", "hit",
    "send", "ask", "idea", "note", "todo", "per", "via", "before", "after", "over", "under", "again", "still", "seem",
    "seems", "need", "needs", "want", "wants", "thing", "things", "call", "first", "maybe", "look", "see", "let", "now",
    "today", "tomorrow", "week", "day", "on", "in", "at", "to", "of", "or", "an", "is", "it", "be", "do", "if", "so", "by",
    "we", "me", "my", "up", "no", "vs", "re",
];

fn stem(w: &str) -> String {
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .map(|w| w.trim_matches('-'))
        .filter(|w| w.len() >= 2 && !STOP.contains(w))
        .map(stem)
        .collect()
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b([A-Z][A-Z0-9]{1,9}-\d+)\b").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)#([\w][\w\-./]*)").unwrap())
}

fn mention_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@([A-Za-z][\w.\-]*)").unwrap())
}

/// First @name in the text.
fn mention(text: &str) -> Option<String> {
    mention_re().captures(text).map(|c| c[1].to_string())
}

impl Index {
    fn load(conn: &Connection, vault: &Path) -> Result<Index> {
        let mut people: Vec<PersonDoc> = conn
            .prepare("SELECT id, display_name FROM people ORDER BY id")?
            .query_map([], |r| Ok(PersonDoc { id: r.get(0)?, name: r.get(1)?, topics: vec![] }))?
            .collect::<rusqlite::Result<_>>()?;
        let names: HashSet<String> = people.iter().flat_map(|p| words(&p.name)).collect();

        let mut topics: Vec<TopicDoc> = vec![];
        let mut stmt = conn.prepare(
            "SELECT id, name, category, obsidian_path FROM topics WHERE archived_at IS NULL ORDER BY coalesce(score, 0) DESC, name",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut items = conn.prepare(
            "SELECT i.title, coalesce(i.summary, ''), i.body FROM topic_links tl JOIN items i ON i.id = tl.item_id
              WHERE tl.topic_id = ?1 AND tl.state != 'wrong'",
        )?;
        let mut tp = conn.prepare("SELECT person_id FROM topic_people WHERE topic_id = ?1")?;
        for (id, name, category, path) in rows {
            let mut text = String::new();
            for r in items.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
                let (a, b, c) = r?;
                text.push_str(&format!("{a}\n{b}\n{c}\n"));
            }
            if let Some(p) = &path {
                text.push_str(&std::fs::read_to_string(vault.join(p)).unwrap_or_default());
            }
            let name_words: HashSet<String> = words(&name).into_iter().filter(|w| !names.contains(w)).collect();
            let words: HashSet<String> = words(&text).into_iter().filter(|w| !names.contains(w)).collect();
            let tpeople: HashSet<i64> = tp.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for p in people.iter_mut().filter(|p| tpeople.contains(&p.id)) {
                p.topics.push(id);
            }
            topics.push(TopicDoc { id, name, category, name_words, words, people: tpeople });
        }

        let mut issues = vec![];
        let mut stmt = conn.prepare(
            "SELECT i.title, (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state = 'confirmed' LIMIT 1)
               FROM items i JOIN sources s ON s.id = i.source_id
              WHERE s.kind = 'jira' AND i.kind = 'issue' ORDER BY i.occurred_at DESC",
        )?;
        for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)))? {
            let (title, topic_id) = r?;
            if let Some(c) = key_re().captures(&title) {
                let key = c[1].to_string();
                let rest = title.replacen(&key, "", 1);
                issues.push(IssueDoc { words: words(&rest).into_iter().filter(|w| w.len() >= 4).collect(), key, title, topic_id });
            }
        }
        Ok(Index { topics, people, issues, names })
    }

    fn topic_target(&self, t: &TopicDoc, reason: Option<String>) -> Target {
        Target {
            ty: "topic".into(),
            id: Some(t.id),
            label: t.name.clone(),
            category: Some(t.category.clone()),
            app: None,
            key: None,
            topic_id: None,
            offer: None,
            create: false,
            auto: true,
            reason,
        }
    }

    fn person_target(&self, p: &PersonDoc) -> Target {
        Target {
            ty: "person".into(),
            id: Some(p.id),
            label: format!("People/{}", p.name),
            category: None,
            app: None,
            key: None,
            // A person who belongs to exactly one topic files there.
            topic_id: if p.topics.len() == 1 { Some(p.topics[0]) } else { None },
            offer: None,
            create: false,
            auto: true,
            reason: None,
        }
    }

    fn issue_target(&self, i: &IssueDoc) -> Target {
        Target {
            ty: "app".into(),
            id: None,
            label: format!("Jira · {}", i.key),
            category: None,
            app: Some("jira".into()),
            key: Some(i.key.clone()),
            topic_id: i.topic_id,
            offer: None,
            create: false,
            auto: true,
            reason: Some(format!("mentions {}", i.key)),
        }
    }

    /// People named in the text, @mentions first.
    fn mentioned(&self, text: &str) -> Vec<&PersonDoc> {
        let lower: HashSet<String> = text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        let at = mention(text).map(|m| m.to_lowercase());
        let mut out: Vec<&PersonDoc> =
            self.people.iter().filter(|p| lower.contains(&p.name.to_lowercase())).collect();
        out.sort_by_key(|p| Some(p.name.to_lowercase()) != at);
        out
    }

    /// Topics scored by word overlap: 3 per topic-name word, 1 per word found in
    /// the topic's items and note, +2 when a named person belongs to the topic.
    fn scores(&self, text: &str) -> Vec<(i64, &TopicDoc, Vec<String>)> {
        let ws: HashSet<String> = words(text).into_iter().filter(|w| !self.names.contains(w)).collect();
        let people: Vec<i64> = self.mentioned(text).iter().map(|p| p.id).collect();
        let mut out = vec![];
        for t in &self.topics {
            let mut score = 0;
            let mut hits = vec![];
            for w in &ws {
                if t.name_words.contains(w) {
                    score += 3;
                    hits.push(w.clone());
                } else if t.words.contains(w) {
                    score += 1;
                    hits.push(w.clone());
                }
            }
            if people.iter().any(|p| t.people.contains(p)) {
                score += 2;
            }
            if score > 0 {
                hits.sort();
                out.push((score, t, hits));
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out
    }

    fn tagged(&self, text: &str) -> Option<&TopicDoc> {
        let tag = norm(&tag_re().captures(text)?[1]);
        if tag.is_empty() {
            return None;
        }
        self.topics
            .iter()
            .find(|t| norm(&t.name) == tag)
            .or_else(|| self.topics.iter().find(|t| norm(&t.name).starts_with(&tag)))
            .or_else(|| self.topics.iter().find(|t| t.name_words.iter().any(|w| norm(w) == tag)))
    }

    fn issue_for(&self, text: &str) -> Option<Target> {
        if let Some(c) = key_re().captures(text) {
            let key = &c[1];
            return Some(match self.issues.iter().find(|i| i.key == key) {
                Some(i) => self.issue_target(i),
                None => Target {
                    ty: "app".into(),
                    id: None,
                    label: format!("Jira · {key}"),
                    category: None,
                    app: Some("jira".into()),
                    key: Some(key.to_string()),
                    topic_id: None,
                    offer: None,
                    create: false,
                    auto: true,
                    reason: Some(format!("mentions {key}")),
                },
            });
        }
        let ws: HashSet<String> = words(text).into_iter().filter(|w| w.len() >= 4 && !self.names.contains(w)).collect();
        self.issues
            .iter()
            .map(|i| (i.words.intersection(&ws).count(), i))
            .filter(|(n, _)| *n > 0)
            .max_by_key(|(n, _)| *n)
            .map(|(_, i)| {
                let mut t = self.issue_target(i);
                t.reason = Some(format!("like {}", i.key));
                t
            })
    }

    /// The one suggested destination for a line, or None ("needs a topic").
    /// Order: #tag, a Jira key or matching Jira issue (not for delegated lines,
    /// which get a topic and an offer instead), best topic, a named person.
    pub fn suggest(&self, kind: &str, text: &str) -> Option<Target> {
        if kind == "life" {
            return Some(marvin());
        }
        if text.trim().is_empty() {
            return None;
        }
        let delegated = kind == "delegated";
        let mut t = self
            .tagged(text)
            .map(|t| self.topic_target(t, Some("#tag".into())))
            .or_else(|| if delegated { None } else { self.issue_for(text) })
            .or_else(|| {
                self.scores(text)
                    .into_iter()
                    .next()
                    .filter(|(s, _, _)| *s >= 2)
                    .map(|(_, t, hits)| self.topic_target(t, Some(format!("matched {}", hits.join(", ")))))
            })
            .or_else(|| self.mentioned(text).first().map(|p| self.person_target(p)));
        if delegated {
            if let Some(t) = &mut t {
                t.offer = Some("create Jira task".into());
            }
        }
        t
    }

    /// Every plausible destination, best first (for the Tab picker).
    fn ranked(&self, kind: &str, text: &str) -> Vec<Target> {
        let mut out = vec![];
        if let Some(t) = self.suggest(kind, text) {
            out.push(t);
        }
        if let Some(t) = self.issue_for(text) {
            out.push(t);
        }
        for (_, t, _) in self.scores(text) {
            out.push(self.topic_target(t, None));
        }
        for p in self.mentioned(text) {
            out.push(self.person_target(p));
        }
        for t in &mut out {
            t.offer = None;
            t.reason = None;
        }
        out
    }
}

// ---------------------------------------------------------------------------

pub fn seed(ctx: &Ctx) -> anyhow::Result<()> {
    let c = ctx.conn;
    // The Jira issue behind "the backoff PR" (9a: → Jira · PLAT-1201).
    let plat = ctx.item(ItemSeed {
        source: "jira",
        kind: "issue",
        title: "PLAT-1201 · Exponential backoff for webhook retries".into(),
        summary: Some("In review · PR open by Marco".into()),
        occurred_at: ctx.at(-2, "15:20"),
        owner: Some("Marco".into()),
        ..Default::default()
    });
    ctx.link(plat, "Webhook reliability", "confirmed");

    let line = |date: NaiveDate, pos: i64, kind: &str, text: &str, state: &str, meta: Value| -> anyhow::Result<()> {
        c.execute(
            "INSERT INTO notes_lines (page_date, position, kind, text, target_json, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![date.to_string(), pos, kind, text, meta.to_string(), state],
        )?;
        Ok(())
    };
    let none = json!({});

    // Today's page (9a / 10a), not sent yet. Destinations are suggested on read.
    let today = ctx.today;
    for (i, (kind, text)) in [
        ("note", "Priya seems worried about staffing, not budget"),
        ("delegated", "@Jordan check whether the retry storm hit other customers"),
        ("do", "send Marco feedback on the backoff PR"),
        ("life", "book dentist cleaning"),
        ("research", "how do other orgs split platform on-call?"),
        ("note", "idea: rotate incident lead weekly, not per incident"),
        ("note", "3y deal only makes sense if logs get fixed first"),
        ("delegated", "@Lee attrition numbers before the headcount call"),
    ]
    .iter()
    .enumerate()
    {
        line(today, i as i64, kind, text, "draft", none.clone())?;
    }

    // Yesterday's page, already sent: shows the 11a states (went to Incoming,
    // held, and the life-line receipt states the Phase 2 Marvin client will drive).
    let y = today - Duration::days(1);
    let topic = |name: &str| {
        json!({ "type": "topic", "id": ctx.topic(name), "label": name, "category": "work", "auto": true })
    };
    line(y, 0, "note", "Kim wants a walk-away number before Thursday", "incoming", topic("Datadog renewal"))?;
    line(y, 1, "research", "what retry budget do other teams use for webhooks?", "incoming", topic("Webhook reliability"))?;
    line(y, 2, "life", "renew parking permit", "confirmed", json!({ "receipt_at": ctx.at(-1, "17:42") }))?;
    line(y, 3, "note", "postmortem template feels too long", "held", none.clone())?;
    line(y, 4, "life", "ask Sam about Saturday pickup", "failed", json!({ "error": "Marvin unreachable" }))?;
    line(y, 5, "do", "block time for Q4 planning doc", "incoming", topic("Q4 planning"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let app = App::from_parts(conn, Settings::default(), dir.path()).unwrap();
        (app, dir)
    }

    fn today_page(app: &App) -> Value {
        call(app, "page", &json!({})).unwrap()
    }

    fn by_text<'a>(page: &'a Value, starts: &str) -> &'a Value {
        page["lines"].as_array().unwrap().iter().find(|l| l["text"].as_str().unwrap().starts_with(starts)).unwrap()
    }

    #[test]
    fn seeded_page_suggests_the_mock_destinations() {
        let (app, _d) = app();
        let p = today_page(&app);
        assert_eq!(p["label"], "Today's page");
        assert_eq!(by_text(&p, "Priya")["target"]["label"], "People/Priya");
        let jordan = &by_text(&p, "@Jordan")["target"];
        assert_eq!(jordan["label"], "Acme outage");
        assert_eq!(jordan["offer"], "create Jira task");
        assert_eq!(by_text(&p, "send Marco")["target"]["label"], "Jira · PLAT-1201");
        assert_eq!(by_text(&p, "how do other orgs")["target"]["label"], "On-call load");
        assert!(by_text(&p, "idea: rotate")["target"].is_null());
        assert_eq!(by_text(&p, "3y deal")["target"]["label"], "Datadog renewal");
        assert_eq!(by_text(&p, "book dentist")["target"]["app"], "marvin");
        assert_eq!(by_text(&p, "@Lee")["target"]["label"], "Q4 headcount");
    }

    #[test]
    fn tag_sets_the_topic() {
        let (app, _d) = app();
        let t = call(&app, "suggest", &json!({ "text": "rotate incident lead weekly #oncall", "kind": "note" })).unwrap();
        assert_eq!(t["label"], "On-call load");
        let t = call(&app, "suggest", &json!({ "text": "follow up #datadog", "kind": "do" })).unwrap();
        assert_eq!(t["label"], "Datadog renewal");
    }

    #[test]
    fn save_round_trips_and_removes_only_editable_lines() {
        let (app, _d) = app();
        let date = app.clock.now().date().to_string();
        let y = (app.clock.now().date() - Duration::days(1)).to_string();
        let r = call(
            &app,
            "save",
            &json!({ "date": date, "lines": [
                { "key": "a", "kind": "note", "bare": true, "text": "first line" },
                { "key": "b", "kind": "do", "text": "second #datadog" },
            ]}),
        )
        .unwrap();
        let lines = r["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["key"], "a");
        assert_eq!(lines[0]["bare"], true);
        assert_eq!(lines[1]["target"]["label"], "Datadog renewal");
        assert_eq!(lines[1]["target"]["auto"], true);

        // Saving yesterday's page with nothing keeps the sent and receipt-tracked lines.
        let before = call(&app, "page", &json!({ "date": y })).unwrap()["lines"].as_array().unwrap().len();
        let after = call(&app, "save", &json!({ "date": y, "lines": [] })).unwrap();
        let left = after["lines"].as_array().unwrap();
        assert_eq!(left.len(), before - 1, "only the held line is removable");
        assert!(left.iter().all(|l| l["locked"] == true));
    }

    #[test]
    fn manual_pick_survives_text_edits() {
        let (app, _d) = app();
        let pick = json!({ "type": "topic", "id": 1, "label": "Acme outage", "category": "work", "auto": false });
        let r = call(&app, "save", &json!({ "lines": [{ "kind": "note", "text": "anything at all", "target": pick }] })).unwrap();
        let id = r["lines"][0]["id"].as_i64().unwrap();
        let r = call(&app, "save", &json!({ "lines": [{ "id": id, "kind": "note", "text": "anything else", "target": pick }] })).unwrap();
        assert_eq!(r["lines"][0]["target"]["label"], "Acme outage");
        assert_eq!(r["lines"][0]["target"]["auto"], false);
    }

    #[test]
    fn send_creates_incoming_signals_and_holds_the_rest() {
        let (app, _d) = app();
        let unseen = |app: &App| -> i64 { app.db().query_row("SELECT count(*) FROM items WHERE seen = 0", [], |r| r.get(0)).unwrap() };
        let before = unseen(&app);
        let p = call(&app, "send", &json!({})).unwrap();
        // 8 lines: 6 with a destination go, 1 is held, the life line stays a draft.
        assert_eq!(unseen(&app) - before, 6);
        assert_eq!(by_text(&p, "idea: rotate")["state"], "held");
        assert_eq!(by_text(&p, "book dentist")["state"], "draft");
        let sent = by_text(&p, "3y deal");
        assert_eq!(sent["state"], "incoming");
        assert_eq!(sent["locked"], true);

        let conn = app.db();
        // Topic lines are filed as suggestions under their topic.
        let (topic, state): (String, String) = conn
            .query_row(
                "SELECT t.name, tl.state FROM items i JOIN topic_links tl ON tl.item_id = i.id JOIN topics t ON t.id = tl.topic_id
                  WHERE i.title LIKE '3y deal%'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((topic.as_str(), state.as_str()), ("Datadog renewal", "suggested"));
        // The Jira-issue line files under the issue's topic.
        let t: String = conn
            .query_row(
                "SELECT t.name FROM items i JOIN topic_links tl ON tl.item_id = i.id JOIN topics t ON t.id = tl.topic_id
                  WHERE i.title LIKE 'send Marco%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t, "Webhook reliability");
        // Priya has no single topic, so her line is unlinked (Unsorted).
        let n: i64 = conn
            .query_row("SELECT count(*) FROM items i JOIN topic_links tl ON tl.item_id = i.id WHERE i.title LIKE 'Priya%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        // Life lines never reach items, and only offers were made (no outbox writes).
        let life: i64 = conn.query_row("SELECT count(*) FROM items WHERE title LIKE '%dentist%'", [], |r| r.get(0)).unwrap();
        assert_eq!(life, 0);
        let ob: i64 = conn.query_row("SELECT count(*) FROM outbox", [], |r| r.get(0)).unwrap();
        assert_eq!(ob, 0);
        drop(conn);

        // Sending again does nothing new.
        call(&app, "send", &json!({})).unwrap();
        assert_eq!(unseen(&app) - before, 6);
    }

    #[test]
    fn accepted_offer_queues_a_jira_write() {
        let (app, _d) = app();
        let o = call(&app, "options", &json!({ "text": "@Jordan check the retry storm", "kind": "delegated" })).unwrap();
        let create = o["create"].clone();
        assert_eq!(create["create"], true);
        call(&app, "save", &json!({ "lines": [{ "kind": "delegated", "text": "@Jordan check the retry storm", "target": create }] })).unwrap();
        call(&app, "send", &json!({})).unwrap();
        let (op, state): (String, String) =
            app.db().query_row("SELECT op, state FROM outbox", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((op.as_str(), state.as_str()), ("jira.create_issue", "pending"));
    }

    #[test]
    fn options_and_search() {
        let (app, _d) = app();
        let o = call(&app, "options", &json!({ "text": "3y deal only makes sense if logs get fixed first", "kind": "note" })).unwrap();
        let opts = o["options"].as_array().unwrap();
        assert_eq!(opts.len(), 3);
        assert!(opts.iter().all(|t| t["label"] != "Datadog renewal"), "current suggestion is not repeated");
        assert!(o["create"].is_null());
        let o = call(&app, "options", &json!({ "text": "x", "q": "pri" })).unwrap();
        assert!(o["options"].as_array().unwrap().iter().any(|t| t["label"] == "People/Priya"));
    }

    #[test]
    fn retry_moves_failed_life_lines_back_to_pending() {
        let (app, _d) = app();
        let y = (app.clock.now().date() - Duration::days(1)).to_string();
        let p = call(&app, "retry", &json!({ "date": y })).unwrap();
        let sam = by_text(&p, "ask Sam");
        assert_eq!(sam["state"], "pending");
        assert!(sam["error"].is_null());
        assert_eq!(by_text(&p, "renew parking")["state"], "confirmed");
    }
}

#[cfg(test)]
mod jira_link_tests {
    use crate::settings::Settings;
    use crate::state::App;
    use serde_json::json;

    #[test]
    fn line_shows_the_created_issue_key_once_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
        let (line, date): (i64, String) =
            app.db().query_row("SELECT id, page_date FROM notes_lines LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let jira: i64 = app.db().query_row("SELECT id FROM sources WHERE kind = 'jira'", [], |r| r.get(0)).unwrap();
        app.db()
            .execute(
                "INSERT INTO outbox (source_id, op, payload_json, state, error) VALUES (?1, 'jira.create_issue', ?2, 'failed', 'boom')",
                rusqlite::params![jira, json!({ "notes_line_id": line }).to_string()],
            )
            .unwrap();
        let find = |p: &serde_json::Value| p["lines"].as_array().unwrap().iter().find(|l| l["id"] == line).unwrap().clone();
        let p = super::call(&app, "page", &json!({ "date": date })).unwrap();
        assert_eq!(find(&p)["jira"]["state"], "failed");
        let p = super::call(&app, "retry", &json!({ "date": date })).unwrap();
        assert_eq!(find(&p)["jira"]["state"], "pending");
        app.db()
            .execute("UPDATE outbox SET state = 'confirmed', receipt_json = ?1", [json!({ "key": "PLAT-77", "url": "u" }).to_string()])
            .unwrap();
        let p = super::call(&app, "page", &json!({ "date": date })).unwrap();
        assert_eq!(find(&p)["jira"]["key"], "PLAT-77");
    }
}

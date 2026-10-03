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
use crate::domain::items::NewItem;
use crate::domain::notes::{Corpus, NoteLine};
use crate::domain::ports::UnitOfWork;
use crate::error::{msg, Result};
use crate::seed::{Ctx, ItemSeed};
use crate::state::App;
use crate::time::{Date, Ts};
use jiff::ToSpan;
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

const KINDS: &[&str] = &["note", "do", "research", "delegated", "life"];
/// States a line can still be edited (and sent) in.
const EDITABLE: &[&str] = &["draft", "held"];

pub use crate::domain::notes::{LineMeta, Target};

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

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "page" => page(app, page_date(app, args)?, &HashMap::new()),
        "save" => {
            let date = page_date(app, args)?;
            let lines: Vec<SaveLine> = serde_json::from_value(args.get("lines").cloned().unwrap_or(json!([])))?;
            let keys = app.store().write(|uow| save(uow, date, &lines))?;
            page(app, date, &keys)
        }
        "send" => {
            let date = page_date(app, args)?;
            // Suggestions are worked out before the write: they read the vault.
            let idx = index(app)?;
            let now = app.clock.now();
            app.store().write(|uow| send(uow, &idx, date, now))?;
            page(app, date, &HashMap::new())
        }
        "suggest" => {
            let idx = index(app)?;
            let text = arg_str(args, "text")?;
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("note");
            Ok(json!(idx.suggest(kind, &text)))
        }
        "options" => options(&index(app)?, args),
        "retry" => {
            let date = page_date(app, args)?;
            app.store().write(|uow| retry(uow, date, opt_i64(args, "id")))?;
            page(app, date, &HashMap::new())
        }
        _ => Err(unknown(name)),
    }
}

/// The suggestion index: the database's part on a snapshot, then the topics' notes from the vault.
fn index(app: &App) -> Result<Index> {
    let corpus = app.store().read(|r| r.notes().corpus())?;
    Ok(Index::build(corpus, &app.vault))
}

fn page_date(app: &App, args: &Value) -> Result<Date> {
    match args.get("date").and_then(Value::as_str) {
        Some(d) if !d.is_empty() => parse_date(d),
        _ => Ok(app.clock.today()),
    }
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
fn effective(idx: &Index, r: &NoteLine) -> Option<Target> {
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

fn page(app: &App, date: Date, keys: &HashMap<i64, String>) -> Result<Value> {
    let today = app.clock.today();
    let (rows, mut jira, corpus) = app.store().read(|r| {
        let rows = r.notes().lines(date)?;
        let ids: Vec<i64> = rows.iter().map(|l| l.id).collect();
        Ok((rows, r.notes().jira_requests(&ids)?, r.notes().corpus()?))
    })?;
    let idx = Index::build(corpus, &app.vault);
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
            jira: jira.remove(&r.id).map(|j| json!({ "state": j.state, "error": j.error, "key": j.key, "url": j.url })),
        })
        .collect();
    let label = match (date - today).get_days() {
        0 => "Today's page".to_string(),
        -1 => "Yesterday".to_string(),
        _ => date.strftime("%a, %b %-d").to_string(),
    };
    Ok(json!({
        "date": date.to_string(),
        "label": label,
        "today": date == today,
        "prev": (date - 1.day()).to_string(),
        "next": if date < today { Some((date + 1.day()).to_string()) } else { None },
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
/// are never deleted here. Returns the client's key for each saved line.
fn save(uow: &dyn UnitOfWork, date: Date, lines: &[SaveLine]) -> Result<HashMap<i64, String>> {
    let notes = uow.note_lines();
    let existing: HashMap<i64, NoteLine> = uow.reads().notes().lines(date)?.into_iter().map(|r| (r.id, r)).collect();
    let mut keep: HashSet<i64> = HashSet::new();
    let mut keys = HashMap::new();
    for (pos, l) in lines.iter().enumerate() {
        let pos = pos as i64;
        if !KINDS.contains(&l.kind.as_str()) {
            return Err(msg(format!("bad kind '{}'", l.kind)));
        }
        let own = |t: &Option<Target>| t.clone().filter(|t| !t.auto && l.kind != "life");
        let id = match l.id.and_then(|id| existing.get(&id)) {
            Some(r) if !EDITABLE.contains(&r.state.as_str()) => {
                // Locked: only its position can change.
                notes.set_position(r.id, pos)?;
                r.id
            }
            Some(r) => {
                let mut meta = r.meta.clone();
                meta.bare = l.bare && l.kind == "note";
                meta.to = own(&l.target).map(|t| Target { auto: false, ..t });
                notes.update(r.id, pos, &l.kind, &l.text, &meta)?;
                r.id
            }
            None => notes.insert(date, pos, &l.kind, &l.text, &LineMeta { to: own(&l.target), bare: l.bare && l.kind == "note", ..Default::default() })?,
        };
        keep.insert(id);
        if let Some(k) = &l.key {
            keys.insert(id, k.clone());
        }
    }
    for r in existing.values() {
        if !keep.contains(&r.id) && EDITABLE.contains(&r.state.as_str()) {
            notes.delete(r.id)?;
        }
    }
    Ok(keys)
}

/// Local work source that Notes signals come from.
pub(crate) fn alethiometer_source(conn: &Connection) -> Result<i64> {
    UnitOfWork::sources(&crate::store::over(conn)).ensure_local("notes", NOTES_SOURCE)
}

const NOTES_SOURCE: &str = "Alethiometer Notes";

/// Each editable work line with a destination becomes a new Incoming signal
/// (an unseen item). Lines with no destination are held. Life lines and
/// "keep as a note" lines stay where they are.
fn send(uow: &dyn UnitOfWork, idx: &Index, date: Date, now: Ts) -> Result<()> {
    let rows = uow.reads().notes().lines(date)?;
    let src = uow.sources().ensure_local("notes", NOTES_SOURCE)?;
    for r in rows {
        if r.kind == "life" || !EDITABLE.contains(&r.state.as_str()) || r.text.trim().is_empty() {
            continue;
        }
        let Some(t) = effective(idx, &r) else {
            uow.note_lines().set_state(r.id, "held", &r.meta)?;
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
        let item = uow.items().insert(&NewItem {
            source_id: src,
            external_id: format!("notes-line-{}", r.id),
            kind: item_kind.into(),
            title: r.text.trim().into(),
            summary: Some(summary),
            occurred_at: now,
            status: "open".into(),
            owner,
            seen: false,
            raw_json: json!({ "notes_line_id": r.id, "page_date": date.to_string(), "kind": r.kind, "target": t }).to_string(),
        })?;
        // Filed under its topic as a suggestion, so it lands in that topic's Incoming
        // list for confirm / wrong-topic. Person targets without a single home topic
        // stay unlinked and show up in Unsorted.
        let topic = match t.ty.as_str() {
            "topic" => t.id,
            _ => t.topic_id,
        };
        if let Some(topic) = topic {
            let reason = t.reason.clone().unwrap_or_else(|| if t.auto { "Notes suggestion".into() } else { "you picked it in Notes".into() });
            uow.links().suggest(item, topic, if t.auto { 0.7 } else { 1.0 }, &reason)?;
        }
        if t.create {
            if let Some(jira) = uow.reads().sources().of_kind("jira")?.first() {
                let payload = json!({ "summary": r.text.trim(), "assignee": delegate, "notes_line_id": r.id, "item_id": item });
                uow.outbox().enqueue(jira.id, "jira.create_issue", &payload.to_string(), Some(item))?;
            }
        }
        let mut meta = r.meta.clone();
        meta.to = Some(Target { auto: t.auto, ..t });
        uow.note_lines().set_state(r.id, "incoming", &meta)?;
    }
    Ok(())
}

/// ⌘R: failed life lines go back to pending. They were never removed.
/// ⌘R: failed life lines go back to pending (they were never removed), and
/// the page's failed Jira task requests are sent again.
fn retry(uow: &dyn UnitOfWork, date: Date, id: Option<i64>) -> Result<()> {
    let rows = uow.reads().notes().lines(date)?;
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    for r in rows.into_iter().filter(|r| r.kind == "life" && r.state == "failed" && id.is_none_or(|i| i == r.id)) {
        let mut meta = r.meta;
        meta.error = None;
        uow.note_lines().set_state(r.id, "pending", &meta)?;
    }
    uow.outbox().retry_line_requests(&ids)?;
    Ok(())
}

/// Picker contents: with no query, the next likely destinations after the
/// current one; with a query, topics / people / Jira keys matching it.
fn options(idx: &Index, args: &Value) -> Result<Value> {
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
    /// From the database's part and each topic's note in the vault.
    fn build(corpus: Corpus, vault: &Path) -> Index {
        let mut people: Vec<PersonDoc> = corpus.people.into_iter().map(|(id, name)| PersonDoc { id, name, topics: vec![] }).collect();
        let names: HashSet<String> = people.iter().flat_map(|p| words(&p.name)).collect();
        let mut topics: Vec<TopicDoc> = vec![];
        for t in corpus.topics {
            let mut text = t.text;
            if let Some(p) = &t.obsidian_path {
                text.push_str(&std::fs::read_to_string(vault.join(p)).unwrap_or_default());
            }
            let name_words: HashSet<String> = words(&t.name).into_iter().filter(|w| !names.contains(w)).collect();
            let words: HashSet<String> = words(&text).into_iter().filter(|w| !names.contains(w)).collect();
            let tpeople: HashSet<i64> = t.people.into_iter().collect();
            for p in people.iter_mut().filter(|p| tpeople.contains(&p.id)) {
                p.topics.push(t.id);
            }
            topics.push(TopicDoc { id: t.id, name: t.name, category: t.category, name_words, words, people: tpeople });
        }
        let mut issues = vec![];
        for (title, topic_id) in corpus.issues {
            if let Some(c) = key_re().captures(&title) {
                let key = c[1].to_string();
                let rest = title.replacen(&key, "", 1);
                issues.push(IssueDoc { words: words(&rest).into_iter().filter(|w| w.len() >= 4).collect(), key, title, topic_id });
            }
        }
        Index { topics, people, issues, names }
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

    let line = |date: Date, pos: i64, kind: &str, text: &str, state: &str, meta: Value| -> anyhow::Result<()> {
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
    let y = today - 1.day();
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
        let date = app.clock.today().to_string();
        let y = (app.clock.today() - 1.day()).to_string();
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
        let y = (app.clock.today() - 1.day()).to_string();
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

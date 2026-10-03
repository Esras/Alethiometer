//! Local embeddings: nothing here leaves the machine.
//!
//! The model (BAAI/bge-small-en-v1.5, ONNX, 384 dimensions) is downloaded
//! from Hugging Face into `<data dir>/models/` on first run, then runs
//! in-process with tract (pure Rust ONNX inference; no native runtime to
//! ship). Vectors live in SQLite through the sqlite-vec extension
//! (`vec_items`, `vec_topics`), with `embed_meta` remembering what text each
//! vector was made from so changed items and topics are re-embedded.
//!
//! Only work items are embedded (boundary rule 3: a personal source stores
//! untitled busy blocks only, so there's nothing to embed anyway; the query
//! filters on the tag regardless).
//!
//! Uses: the off-topic check for Glean results (similarity to the anchor
//! topic) and the local route of hydrated search (items near a topic).

pub mod model;

use crate::error::{msg, Result};
use crate::state::App;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};

pub const DIM: usize = 384;
pub const MODEL_ID: &str = "bge-small-en-v1.5";

/// Turns text into unit-length vectors.
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn id(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Model files aren't here yet.
    #[default]
    Missing,
    Downloading { done: u64, total: Option<u64> },
    Loading,
    Ready,
    Failed { error: String },
}

/// The process's embedder, once loaded, and its status.
#[derive(Default)]
pub struct Engine {
    pub status: Mutex<Status>,
    embedder: OnceLock<Arc<dyn Embedder>>,
}

impl Engine {
    pub fn get(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.get().cloned()
    }

    /// Tests and the stub: use this embedder from now on.
    pub fn install(&self, e: Arc<dyn Embedder>) {
        let _ = self.embedder.set(e);
        self.set(Status::Ready);
    }

    pub fn set(&self, s: Status) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// ------------------------------------------------------------- storage

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    // Both are unit length.
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn hash(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn table(kind: &str) -> &'static str {
    if kind == "topic" {
        "vec_topics"
    } else {
        "vec_items"
    }
}

/// Store one vector, replacing any earlier one.
pub fn store(conn: &Connection, kind: &str, id: i64, model: &str, text_hash: &str, v: &[f32]) -> Result<()> {
    let t = table(kind);
    conn.execute(&format!("DELETE FROM {t} WHERE rowid = ?1"), [id])?;
    conn.execute(&format!("INSERT INTO {t} (rowid, embedding) VALUES (?1, ?2)"), params![id, to_blob(v)])?;
    conn.execute(
        "INSERT INTO embed_meta (kind, id, model, hash) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (kind, id) DO UPDATE SET model = excluded.model, hash = excluded.hash, at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![kind, id, model, text_hash],
    )?;
    Ok(())
}

pub fn vector(conn: &Connection, kind: &str, id: i64) -> Result<Option<Vec<f32>>> {
    let blob: Option<Vec<u8>> = conn.query_row(&format!("SELECT embedding FROM {} WHERE rowid = ?1", table(kind)), [id], |r| r.get(0)).optional()?;
    Ok(blob.map(|b| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()))
}

/// Work items nearest to `v`: (item id, cosine similarity), best first.
pub fn nearest_items(conn: &Connection, v: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT v.rowid, v.distance FROM vec_items v
          WHERE v.embedding MATCH ?1 AND k = ?2 ORDER BY v.distance",
    )?;
    let rows = stmt.query_map(params![to_blob(v), k as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?;
    // vec0 reports L2 distance; for unit vectors cos = 1 - d²/2.
    Ok(rows.filter_map(|r| r.ok()).map(|(id, d)| (id, 1.0 - (d * d / 2.0) as f32)).collect())
}

// ------------------------------------------------------------- what gets embedded

/// An item's text: title, summary and the start of its body.
pub fn item_text(title: &str, summary: &str, body: &str) -> String {
    let body: String = body.chars().take(1200).collect();
    format!("{title}\n{summary}\n{body}").trim().to_string()
}

/// A topic's text: its name, the titles of its recent confirmed signals and
/// the start of its note.
pub fn topic_text(conn: &Connection, topic_id: i64, vault: &std::path::Path) -> Result<String> {
    let (name, note): (String, Option<String>) = conn.query_row("SELECT name, obsidian_path FROM topics WHERE id = ?1", [topic_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let titles: Vec<String> = conn
        .prepare(
            "SELECT i.title FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
              WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'
              ORDER BY i.occurred_at DESC LIMIT 20",
        )?
        .query_map([topic_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let note_text = note
        .and_then(|p| std::fs::read_to_string(vault.join(p)).ok())
        .map(|t| t.chars().take(1500).collect::<String>())
        .unwrap_or_default();
    Ok(format!("{name}\n{}\n{note_text}", titles.join("\n")).trim().to_string())
}

/// Embed whatever's missing or stale, `batch` texts at a time. Returns how many were embedded.
pub fn index_pass(app: &App, e: &dyn Embedder, batch: usize) -> Result<usize> {
    let todo: Vec<(String, i64, String)> = {
        let conn = app.db();
        let mut out = vec![];
        let mut stmt = conn.prepare(
            "SELECT i.id, i.title, coalesce(i.summary, ''), coalesce(i.body, ''), m.hash, m.model
               FROM items i JOIN sources s ON s.id = i.source_id
               LEFT JOIN embed_meta m ON m.kind = 'item' AND m.id = i.id
              WHERE s.tag = 'work'
              ORDER BY i.occurred_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?))
        })?;
        for row in rows {
            let (id, title, summary, body, h, model) = row?;
            let text = item_text(&title, &summary, &body);
            if h.as_deref() != Some(&hash(&text)) || model.as_deref() != Some(e.id()) {
                out.push(("item".to_string(), id, text));
            }
            if out.len() >= batch {
                break;
            }
        }
        if out.len() < batch {
            let topics: Vec<i64> = conn.prepare("SELECT id FROM topics WHERE archived_at IS NULL")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for id in topics {
                let text = topic_text(&conn, id, &app.vault)?;
                let stored: Option<(String, String)> = conn.query_row("SELECT hash, model FROM embed_meta WHERE kind = 'topic' AND id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                if stored.as_ref().map(|(h, m)| (h.as_str(), m.as_str())) != Some((&hash(&text), e.id())) {
                    out.push(("topic".to_string(), id, text));
                }
                if out.len() >= batch {
                    break;
                }
            }
        }
        out
    };
    if todo.is_empty() {
        return Ok(0);
    }
    let texts: Vec<String> = todo.iter().map(|(_, _, t)| t.clone()).collect();
    // Embedding is the slow part: no database lock held.
    let vecs = e.embed(&texts).map_err(|x| msg(format!("embedding: {x}")))?;
    let conn = app.db();
    let tx = conn.unchecked_transaction()?;
    for ((kind, id, text), v) in todo.iter().zip(vecs) {
        store(&tx, kind, *id, e.id(), &hash(text), &v)?;
    }
    tx.commit()?;
    Ok(todo.len())
}

/// Embed one text now (a query, a Glean result). None while the model isn't ready.
pub fn embed_now(app: &App, texts: &[String]) -> Option<Vec<Vec<f32>>> {
    let e = app.embed.get()?;
    e.embed(texts).map_err(|x| eprintln!("embedding: {x}")).ok()
}

/// A topic's vector, embedding it first if it's missing.
pub fn topic_vector(app: &App, topic_id: i64) -> Option<Vec<f32>> {
    if let Ok(Some(v)) = vector(&app.db(), "topic", topic_id) {
        return Some(v);
    }
    let text = topic_text(&app.db(), topic_id, &app.vault).ok()?;
    let v = embed_now(app, &[text.clone()])?.into_iter().next()?;
    let id = app.embed.get()?.id().to_string();
    let _ = store(&app.db(), "topic", topic_id, &id, &hash(&text), &v);
    Some(v)
}

// ------------------------------------------------------------- background

/// Load (downloading first if needed) and keep the index current.
pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("embeddings".into())
        .spawn(move || {
            if app.embed.get().is_none() {
                if let Err(e) = model::ensure(&app) {
                    eprintln!("embeddings: {e}");
                    app.embed.set(Status::Failed { error: e.to_string() });
                    return;
                }
            }
            loop {
                let Some(e) = app.embed.get() else { return };
                match index_pass(&app, e.as_ref(), 16) {
                    Ok(n) if n > 0 => continue,
                    Ok(_) => {}
                    Err(err) => eprintln!("embeddings: {err}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        })
        .ok();
}

pub fn call(app: &App, name: &str, _args: &Value) -> Result<Value> {
    match name {
        "status" => {
            let conn = app.db();
            let items: i64 = conn.query_row("SELECT count(*) FROM embed_meta WHERE kind = 'item'", [], |r| r.get(0))?;
            let work: i64 = conn.query_row("SELECT count(*) FROM items i JOIN sources s ON s.id = i.source_id WHERE s.tag = 'work'", [], |r| r.get(0))?;
            Ok(json!({ "status": app.embed.status(), "model": MODEL_ID, "indexed": items, "items": work, "dir": model::dir(app) }))
        }
        "download" => {
            if matches!(app.embed.status(), Status::Downloading { .. } | Status::Loading | Status::Ready) {
                return Ok(json!({ "status": app.embed.status() }));
            }
            let a = app.arc().ok_or_else(|| msg("not running as the app"))?;
            start(a);
            Ok(json!({ "status": app.embed.status() }))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ai::context::tests::app;

    /// Deterministic bag-of-words vectors: shared words → closer.
    pub struct Stub;

    impl Embedder for Stub {
        fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0f32; DIM];
                    for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
                        let mut h: usize = 5381;
                        for b in w.bytes() {
                            h = h.wrapping_mul(33) ^ b as usize;
                        }
                        v[h % DIM] += 1.0;
                    }
                    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                    v.iter().map(|x| x / n).collect()
                })
                .collect())
        }
        fn id(&self) -> &str {
            "stub"
        }
    }

    #[test]
    fn index_then_find_nearest() {
        let (app, _dir) = app();
        app.embed.install(Arc::new(Stub));
        let mut total = 0;
        loop {
            let n = index_pass(&app, &Stub, 50).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        assert!(total > 10, "sample items and topics got vectors ({total})");
        assert_eq!(index_pass(&app, &Stub, 50).unwrap(), 0, "nothing stale on a second pass");
        let q = Stub.embed(&["Datadog renewal counter offer".into()]).unwrap().remove(0);
        let near = nearest_items(&app.db(), &q, 5).unwrap();
        let title: String = app.db().query_row("SELECT title FROM items WHERE id = ?1", [near[0].0], |r| r.get(0)).unwrap();
        assert!(title.to_lowercase().contains("renewal") || title.to_lowercase().contains("datadog") || title.to_lowercase().contains("counter"), "{title}");
        assert!(near[0].1 > near[4].1);
        // An edited item is re-embedded (and its topic, whose text lists signal titles).
        let before = vector(&app.db(), "item", near[0].0).unwrap().unwrap();
        app.db().execute("UPDATE items SET title = 'Something else entirely' WHERE id = ?1", [near[0].0]).unwrap();
        assert!(index_pass(&app, &Stub, 50).unwrap() >= 1);
        assert_ne!(vector(&app.db(), "item", near[0].0).unwrap().unwrap(), before);
    }

    #[test]
    fn personal_items_are_never_embedded() {
        let (app, _dir) = app();
        let topic = crate::ai::context::tests::topic_id(&app, "Datadog renewal");
        let planted = crate::ai::context::tests::plant_personal(&app, topic);
        while index_pass(&app, &Stub, 100).unwrap() > 0 {}
        assert!(vector(&app.db(), "item", planted).unwrap().is_none());
    }
}

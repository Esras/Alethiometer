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
use crate::domain::embed::{Stored, TopicSource, VecKind};
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

// ------------------------------------------------------------- what gets embedded

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

/// Whether `stored` was made from other text or by another model.
fn stale(stored: &Option<Stored>, text: &str, model: &str) -> bool {
    stored.as_ref().map(|s| (s.hash.as_str(), s.model.as_str())) != Some((hash(text).as_str(), model))
}

/// An item's text: title, summary and the start of its body.
pub fn item_text(title: &str, summary: &str, body: &str) -> String {
    let body: String = body.chars().take(1200).collect();
    format!("{title}\n{summary}\n{body}").trim().to_string()
}

/// A topic's text: its name, the titles of its recent confirmed signals and
/// the start of its note. Read the note with no snapshot open.
fn topic_text(t: &TopicSource, vault: &std::path::Path) -> String {
    let note: String = t
        .note_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(vault.join(p)).ok())
        .map(|n| n.chars().take(1500).collect())
        .unwrap_or_default();
    format!("{}\n{}\n{note}", t.name, t.titles.join("\n")).trim().to_string()
}

/// Embed whatever's missing or stale, `batch` texts at a time. Returns how many were embedded.
pub fn index_pass(app: &App, e: &dyn Embedder, batch: usize) -> Result<usize> {
    // Items and topics from one snapshot; topic notes are read after it.
    let (mut todo, topics) = app.store().read(|r| {
        let mut out = vec![];
        r.vectors().each_work_item(&mut |i| {
            let text = item_text(&i.title, &i.summary, &i.body);
            if stale(&i.stored, &text, e.id()) {
                out.push((VecKind::Item, i.id, text));
            }
            out.len() < batch
        })?;
        let topics = if out.len() < batch { r.vectors().active_topics()? } else { vec![] };
        Ok((out, topics))
    })?;
    for t in topics {
        if todo.len() >= batch {
            break;
        }
        let text = topic_text(&t, &app.vault);
        if stale(&t.stored, &text, e.id()) {
            todo.push((VecKind::Topic, t.id, text));
        }
    }
    if todo.is_empty() {
        return Ok(0);
    }
    let texts: Vec<String> = todo.iter().map(|(_, _, t)| t.clone()).collect();
    // Embedding is the slow part: no database held.
    let vecs = e.embed(&texts).map_err(|x| msg(format!("embedding: {x}")))?;
    app.store().write(|uow| {
        for ((kind, id, text), v) in todo.iter().zip(vecs) {
            uow.vectors().store(*kind, *id, e.id(), &hash(text), &v)?;
        }
        Ok(())
    })?;
    Ok(todo.len())
}

/// Embed one text now (a query, a Glean result). None while the model isn't ready.
pub fn embed_now(app: &App, texts: &[String]) -> Option<Vec<Vec<f32>>> {
    let e = app.embed.get()?;
    e.embed(texts).map_err(|x| eprintln!("embedding: {x}")).ok()
}

/// A topic's vector, embedding it first if it's missing.
pub fn topic_vector(app: &App, topic_id: i64) -> Option<Vec<f32>> {
    if let Some(v) = app.store().read(|r| r.vectors().vector(VecKind::Topic, topic_id)).ok()? {
        return Some(v);
    }
    let source = app.store().read(|r| r.vectors().topic_source(topic_id)).ok()??;
    let text = topic_text(&source, &app.vault);
    let e = app.embed.get()?;
    let v = e.embed(&[text.clone()]).map_err(|x| eprintln!("embedding: {x}")).ok()?.pop()?;
    let _ = app.store().write(|uow| uow.vectors().store(VecKind::Topic, topic_id, e.id(), &hash(&text), &v));
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
            let (items, work) = app.store().read(|r| r.vectors().counts())?;
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

    fn item_vector(app: &App, id: i64) -> Option<Vec<f32>> {
        app.store().read(|r| r.vectors().vector(VecKind::Item, id)).unwrap()
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
        let near = app.store().read(|r| r.vectors().nearest_items(&q, 5)).unwrap();
        let title: String = app.db().query_row("SELECT title FROM items WHERE id = ?1", [near[0].0], |r| r.get(0)).unwrap();
        assert!(title.to_lowercase().contains("renewal") || title.to_lowercase().contains("datadog") || title.to_lowercase().contains("counter"), "{title}");
        assert!(near[0].1 > near[4].1);
        // An edited item is re-embedded (and its topic, whose text lists signal titles).
        let before = item_vector(&app, near[0].0).unwrap();
        app.db().execute("UPDATE items SET title = 'Something else entirely' WHERE id = ?1", [near[0].0]).unwrap();
        assert!(index_pass(&app, &Stub, 50).unwrap() >= 1);
        assert_ne!(item_vector(&app, near[0].0).unwrap(), before);
    }

    #[test]
    fn personal_items_are_never_embedded() {
        let (app, _dir) = app();
        let topic = crate::ai::context::tests::topic_id(&app, "Datadog renewal");
        let planted = crate::ai::context::tests::plant_personal(&app, topic);
        while index_pass(&app, &Stub, 100).unwrap() > 0 {}
        assert!(item_vector(&app, planted).is_none());
    }
}

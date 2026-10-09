//! Embedding vectors for work items and topics, and what they were made from.

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VecKind {
    Item,
    Topic,
}

impl VecKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VecKind::Item => "item",
            VecKind::Topic => "topic",
        }
    }
}

/// The text hash and model of a stored vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub hash: String,
    pub model: String,
}

/// A work item's embeddable fields.
#[derive(Debug, Clone)]
pub struct ItemSource {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub stored: Option<Stored>,
}

/// What a topic's text is made from; the note is read by the caller.
#[derive(Debug, Clone)]
pub struct TopicSource {
    pub id: i64,
    pub name: String,
    pub note_path: Option<String>,
    /// Titles of its latest confirmed work signals, newest first.
    pub titles: Vec<String>,
    pub stored: Option<Stored>,
}

pub trait VectorViews {
    fn vector(&self, kind: VecKind, id: i64) -> Result<Option<Vec<f32>>>;
    /// Work items nearest to `v`: (item id, cosine similarity), best first.
    fn nearest_items(&self, v: &[f32], k: usize) -> Result<Vec<(i64, f32)>>;
    /// Work items queued for embedding (`embed_dirty`), newest first, with
    /// the queue generation read (`Vectors::dequeue`).
    fn queued_items(&self, limit: usize) -> Result<Vec<(ItemSource, i64)>>;
    fn topic_source(&self, id: i64) -> Result<Option<TopicSource>>;
    /// Topics not archived, by id.
    fn active_topics(&self) -> Result<Vec<TopicSource>>;
    /// (items with a vector, work items).
    fn counts(&self) -> Result<(i64, i64)>;
}

pub trait Vectors {
    /// Store one vector, replacing any earlier one.
    fn store(&self, kind: VecKind, id: i64, model: &str, text_hash: &str, v: &[f32]) -> Result<()>;
    /// Take an item off the embedding queue, unless it changed again since
    /// `gen` was read.
    fn dequeue(&self, item_id: i64, gen: i64) -> Result<()>;
    /// Queue items embedded by another model. Returns how many.
    fn requeue_other_models(&self, model: &str) -> Result<usize>;
}

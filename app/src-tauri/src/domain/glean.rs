//! What Glean search and Ask Glean read locally: a topic's entities, local
//! evidence for a question, and the pins and rejects you gave results.

use crate::domain::search::HitTopic;
use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::Ts;
use std::collections::HashMap;

/// A confirmed work signal's fields that name things (Jira keys, channels, links).
#[derive(Debug, Clone)]
pub struct EntitySignal {
    pub title: String,
    pub url: String,
    pub channel: String,
}

/// A work or personal item as local evidence (the caller checks the tag).
#[derive(Debug, Clone)]
pub struct EvidenceItem {
    pub title: String,
    pub url: Option<String>,
    pub source_kind: String,
    pub tag: String,
    pub summary: String,
    pub body: String,
    pub at: Ts,
}

/// A work item matching the search, for the local route.
#[derive(Debug, Clone)]
pub struct LocalHit {
    pub id: i64,
    pub title: String,
    pub url: Option<String>,
    pub source_kind: String,
    /// Last activity.
    pub at: Ts,
    pub summary: Option<String>,
    /// Matches between `\u{1}` and `\u{2}`.
    pub snippet: String,
    /// Top-level messages.
    pub msgs: i64,
}

pub trait GleanViews {
    fn entity_signals(&self, topic_id: i64) -> Result<Vec<EntitySignal>>;
    /// A person's first email account.
    fn email_of(&self, person_id: i64) -> Result<Option<String>>;
    /// A topic's work signals, confirmed or suggested and unseen, newest first.
    fn recent_signals(&self, topic_id: i64, limit: usize) -> Result<Vec<(i64, String)>>;
    /// Items of any tag matching a full-text query, best first.
    fn fts_items(&self, fts: &str, limit: usize) -> Result<Vec<i64>>;
    fn evidence_item(&self, id: i64) -> Result<Option<EvidenceItem>>;
    /// A topic the item is confirmed in.
    fn confirmed_topic(&self, item_id: i64) -> Result<Option<TopicRef>>;
    /// Work items matching a full-text query, best first.
    fn local_hits(&self, fts: &str, limit: usize) -> Result<Vec<LocalHit>>;
    /// Confirmed, else the latest suggested, among topics not archived.
    fn hit_topic(&self, item_id: i64) -> Result<Option<HitTopic>>;
    /// Whether a People note has this name (any case).
    fn has_person_named(&self, name: &str) -> Result<bool>;
    /// A topic's pins and rejects: key → (verdict, when).
    fn feedback(&self, topic_id: i64) -> Result<HashMap<String, (String, Ts)>>;
}

pub trait SearchFeedback {
    /// Pin or reject a result in a topic, replacing an earlier verdict.
    fn set(&self, topic_id: i64, key: &str, verdict: &str, title: &str, url: &str, source: &str) -> Result<()>;
    fn clear(&self, topic_id: i64, key: &str) -> Result<()>;
}

//! Sample data: what a fresh install shows before sources are connected, and
//! the tests' fixtures (`crate::seed`). Writes rows the way sources and the
//! app would have, so nothing here is used once real data exists.

use crate::error::Result;
use crate::time::{Due, Ts};

#[derive(Debug, Clone, Default)]
pub struct SampleItem {
    pub source_id: i64,
    pub external_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub author_id: Option<i64>,
    pub occurred_at: Ts,
    pub ended_at: Option<Ts>,
    pub due: Option<Due>,
    pub status: String,
    pub msg_count: Option<i64>,
    pub owner: Option<String>,
    pub seen: bool,
}

/// Changes to a sample item; `None` leaves a field as it is.
#[derive(Debug, Clone, Default)]
pub struct ItemPatch<'a> {
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub url: Option<&'a str>,
    pub raw_json: Option<&'a str>,
}

/// A message in a sample item.
#[derive(Debug, Clone, Default)]
pub struct SampleMessage<'a> {
    pub item_id: i64,
    pub at: Ts,
    pub author: &'a str,
    pub text: &'a str,
    pub author_id: Option<&'a str>,
    pub ext_key: Option<&'a str>,
    pub reply_to: Option<&'a str>,
}

/// A calendar event: a work meeting with its item, or an untitled Life block.
#[derive(Debug, Clone, Default)]
pub struct SampleEvent<'a> {
    pub item_id: Option<i64>,
    pub source_id: i64,
    pub start: Ts,
    pub end: Ts,
    pub title: Option<&'a str>,
    pub category: &'a str,
    pub tentative: bool,
    pub attendees: Option<&'a str>,
    pub life: bool,
}

pub trait SampleData {
    /// Whether any source exists (then the database isn't new).
    fn has_sources(&self) -> Result<bool>;
    fn source(&self, kind: &str, tag: &str, name: &str) -> Result<i64>;
    fn set_cursor(&self, source_id: i64, cursor: &str) -> Result<()>;
    fn person(&self, name: &str, role: &str) -> Result<i64>;
    /// A person's People note and the accounts it lists.
    fn person_note(&self, person_id: i64, note_path: &str, accounts: &[(&str, &str)]) -> Result<()>;
    fn profile(&self, kind: &str, account: &str, real_name: &str, email: &str) -> Result<()>;
    fn topic(&self, name: &str, category: &str, note_path: &str) -> Result<i64>;
    fn topic_score(&self, topic_id: i64, score: f64, pills_json: &str) -> Result<()>;
    fn topic_person(&self, topic_id: i64, person_id: i64) -> Result<()>;
    fn topic_repo(&self, topic_id: i64, repo: &str) -> Result<()>;
    fn item(&self, it: &SampleItem) -> Result<i64>;
    fn patch_item(&self, id: i64, p: &ItemPatch) -> Result<()>;
    fn link(&self, item_id: i64, topic_id: i64, state: &str) -> Result<()>;
    fn message(&self, m: &SampleMessage) -> Result<i64>;
    fn event(&self, e: &SampleEvent) -> Result<()>;
    fn priority(&self, item_id: i64, quadrant: &str, score: f64, reasons_json: &str) -> Result<()>;
    fn note_line(&self, page_date: &str, position: i64, kind: &str, text: &str, target_json: &str, state: &str) -> Result<()>;
    fn carry(&self, item_id: i64, period: &str, action: Option<&str>) -> Result<()>;
    fn reflection(&self, scope: &str, period_start: &str, note_path: &str, summary: Option<&str>) -> Result<()>;
}

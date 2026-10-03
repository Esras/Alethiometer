//! Links between items and topics: suggested, confirmed or wrong.

use crate::error::Result;

pub trait Links {
    /// The link's state (suggested, confirmed, wrong), if there is one.
    fn state(&self, item_id: i64, topic_id: i64) -> Result<Option<String>>;
    /// Change an existing link's state. False when there's no link.
    fn set_state(&self, item_id: i64, topic_id: i64, state: &str) -> Result<bool>;
    /// Confirm `topic_id` for each item: a new link, or an existing one of any state.
    fn confirm(&self, item_ids: &[i64], topic_id: i64, reason: &str) -> Result<()>;
    /// A confirmed link unless the item already has one to this topic (of any state).
    fn add_confirmed(&self, item_id: i64, topic_id: i64, reason: &str) -> Result<()>;
    /// The user's pick: the link in `state`, as their choice (confidence 1, reason "picked by you").
    fn pick(&self, item_id: i64, topic_id: i64, state: &str) -> Result<()>;
    /// Put a link's state back as it was (None: no link), for undo.
    fn restore(&self, item_id: i64, topic_id: i64, state: Option<&str>) -> Result<()>;
    /// Every live link of the item becomes wrong.
    fn wrong_all(&self, item_id: i64) -> Result<()>;
    /// A source's items with no link yet, from a repo linked to exactly one
    /// topic (not archived), are suggested for it. How many were.
    fn suggest_from_repos(&self, source_id: i64) -> Result<usize>;
    /// Suggest `topic_id` for `item_id`.
    fn suggest(&self, item_id: i64, topic_id: i64, confidence: f64, reason: &str) -> Result<()>;
}

//! Links between items and topics: suggested, confirmed or wrong.

use crate::error::Result;

pub trait Links {
    /// Suggest `topic_id` for `item_id`.
    fn suggest(&self, item_id: i64, topic_id: i64, confidence: f64, reason: &str) -> Result<()>;
}

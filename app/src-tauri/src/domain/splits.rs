//! Split families: an item whose messages were divided among parts, each
//! with its own topic (`queries::split` has the rules).

use crate::error::Result;
use serde_json::Value;
use std::collections::HashSet;

/// FNV-1a, 64 bit: stable across builds (std's hasher isn't promised to be).
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A message's key when its source gives none: a hash of time, author and text.
pub fn content_key(at: &str, author: &str, text: &str) -> String {
    format!("h:{:016x}", fnv(&format!("{at}\u{1f}{author}\u{1f}{text}")))
}

/// One key per message, in order: `raw_json.msgs` when it lists exactly one
/// string per message, otherwise a hash of time, author and text.
pub fn keys(raw: &Value, msgs: &[(&str, &str, &str)]) -> Vec<String> {
    let ids: Option<Vec<&str>> = raw.get("msgs").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect());
    match ids {
        Some(ids) if ids.len() == msgs.len() => ids.into_iter().map(str::to_owned).collect(),
        _ => msgs.iter().map(|(at, author, text)| content_key(at, author, text)).collect(),
    }
}

/// An item and its messages, divided into parts. Each method is one change
/// to the family; none commits.
pub trait Splits {
    /// The item's own top-level messages, oldest first. Replies aren't
    /// listed: they belong to (and move with) their parent.
    fn message_ids(&self, item_id: i64) -> Result<Vec<i64>>;
    /// Move `message_ids` to a new part; returns it. The part starts with the
    /// item's `seen` state and only its `wrong` links. `allow_all` lets it
    /// take every message (the split review moves whole parts).
    fn carve(&self, item_id: i64, message_ids: &[i64], allow_all: bool) -> Result<i64>;
    /// Move messages (and their replies) between parts of one family; returns how many.
    fn move_messages(&self, from: i64, message_ids: &[i64], to: i64) -> Result<usize>;
    /// Remove an emptied part; returns whether it went.
    fn prune(&self, item_id: i64) -> Result<bool>;
    /// Bring an item's derived fields in line with the messages it holds
    /// after `removed` of them went elsewhere.
    fn recount(&self, item_id: i64, removed: usize) -> Result<()>;
    /// Keys of messages that split children of this (source, external id) own.
    fn owned_keys(&self, source_id: i64, external_id: &str) -> Result<HashSet<String>>;
}

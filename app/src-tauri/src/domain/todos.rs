//! Todos.

use crate::error::Result;
use serde::Serialize;

/// A todo waiting on a signal in Incoming, made when the signal is filed.
#[derive(Debug, Serialize, Clone)]
pub struct Held {
    pub target: String,
    pub text: String,
    pub due: Option<String>,
}

pub trait TodoViews {
    /// The todos held on these items, by item.
    fn held(&self, item_ids: &[i64]) -> Result<Vec<(i64, Held)>>;
}

//! Sources: each configured connector's row and its sync state.

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub enabled: bool,
    /// The connector's settings, as JSON.
    pub config_json: String,
    /// The connector's own sync state, as JSON.
    pub cursor: Option<String>,
}

pub trait SourceViews {
    /// Sources of a kind, in id order.
    fn of_kind(&self, kind: &str) -> Result<Vec<SourceRow>>;
}

pub trait Sources {
    /// The app's own work source of `kind` ("notes"), made if missing.
    fn ensure_local(&self, kind: &str, name: &str) -> Result<i64>;
}

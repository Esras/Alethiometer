//! Topic categories, mirrored from settings.json.

use crate::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Category {
    pub key: String,
    pub label: String,
    pub color: String,
}

pub trait CategoryViews {
    /// In the user's order.
    fn list(&self) -> Result<Vec<Category>>;
    /// Topics and calendar blocks in the category.
    fn uses(&self, key: &str) -> Result<(i64, i64)>;
}

pub trait Categories {
    /// The whole list, in this order.
    fn replace(&self, rows: &[Category]) -> Result<()>;
    /// Added at the end.
    fn add(&self, c: &Category) -> Result<()>;
    /// False when there's no such category.
    fn update(&self, key: &str, label: Option<&str>, color: Option<&str>) -> Result<bool>;
    fn delete(&self, key: &str) -> Result<()>;
}

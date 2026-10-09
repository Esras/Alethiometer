//! Topic categories, mirrored from settings.json.

use crate::error::{msg, Result};
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

/// The categories in settings.json, cleaned up, or why they can't be used.
/// Part of `Settings::validate`, so a bad list never gets as far as `load`.
pub fn check(rows: &[Category]) -> Result<Vec<Category>> {
    let mut keep: Vec<Category> = vec![];
    for c in rows {
        let key = c.key.trim();
        if key.is_empty() || keep.iter().any(|k| k.key == key) {
            return Err(msg(format!(
                "settings.json categories: key '{key}' is empty or repeated"
            )));
        }
        keep.push(Category {
            key: key.into(),
            label: label(&c.label)?,
            color: color(&c.color)?,
        });
    }
    Ok(keep)
}

pub fn color(s: &str) -> Result<String> {
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return Err(msg(format!("color must look like #a1b2c3, not '{s}'")));
    }
    Ok(s.to_ascii_lowercase())
}

pub fn label(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(msg("category name can't be empty"));
    }
    Ok(s.to_string())
}

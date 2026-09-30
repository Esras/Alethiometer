//! `settings.json`: sources, categories, buffers, AI allow-list. Hand-edited, except `categories`, which the app also writes back.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub key: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyCategory {
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Categories {
    List(Vec<Category>),
    Legacy(BTreeMap<String, LegacyCategory>),
}

impl<'de> Deserialize<'de> for Categories {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let out = if v.is_array() { serde_json::from_value(v).map(Categories::List) } else { serde_json::from_value(v).map(Categories::Legacy) };
        out.map_err(|e| serde::de::Error::custom(format!("categories: {e}")))
    }
}

/// One configured source. `kind` picks the connector; `name` must be unique per
/// kind. Secrets are NOT stored here: `secret` names a key in the Keychain (or
/// `secrets.json` on Linux).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub kind: String,
    /// "work" or "personal". Personal sources may only produce untitled busy blocks.
    pub tag: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Seconds between polls. Ignored by live sources (Slack Socket Mode, file watcher).
    #[serde(default = "default_poll")]
    pub poll_secs: u64,
    /// Connector-specific settings (base URL, calendar id, repos, vault path…).
    #[serde(default)]
    pub config: serde_json::Value,
}

fn yes() -> bool {
    true
}

fn default_poll() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTier {
    pub provider: String,
    pub model: String,
    /// Reasoning effort: "low", "medium", "high", or "" for the model's default.
    #[serde(default)]
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    /// Providers covered by a BAA (or local). Work data may only go to these.
    pub allowed_providers: Vec<String>,
    /// AWS region for Bedrock.
    pub region: String,
    /// Named profile in `~/.aws/credentials` / `~/.aws/config`. Empty = `AWS_PROFILE`, then "default".
    /// A setting because apps started from the Dock don't see shell env vars.
    #[serde(default)]
    pub aws_profile: String,
    pub small: AiTier,
    pub large: AiTier,
    #[serde(default)]
    pub prices: BTreeMap<String, Price>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Topic categories in UI order. Topics and calendar blocks reference keys, which never change.
    pub categories: Categories,
    /// Day strip range, whole hours.
    pub day_start_hour: u32,
    pub day_end_hour: u32,
    /// Default travel buffer before a Life block.
    pub life_buffer_min: i64,
    /// Absolute path to the work Obsidian vault. Empty = `<data dir>/vault`.
    pub obsidian_vault: String,
    /// Proton Calendar "Limited view" share links, one per calendar. Busy times only.
    pub life_calendars: Vec<String>,
    pub ai: AiSettings,
    /// Development only: pretend the local time is this ("10:52"). Empty = real clock.
    pub fake_now: String,
    /// Page zoom for the desktop window, e.g. 1.15.
    pub ui_scale: f64,
    /// Topics with no signal for this many days sit under "Quiet" in the index.
    pub quiet_days: i64,
    /// Real sources to sync. Empty while running on sample data.
    pub sources: Vec<SourceConfig>,
    /// Fill an empty database with the sample data from the design mocks.
    /// Turn off once real sources are connected.
    pub demo_data: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let cat = |key: &str, label: &str, color: &str| Category { key: key.into(), label: label.into(), color: color.into() };
        Settings {
            categories: Categories::List(vec![
                cat("work", "Important · Work", "#fabd2f"),
                cat("life", "Personal · Life", "#83a598"),
                cat("jobs", "Jobs", "#b8bb26"),
                cat("self", "Self-hold", "#d3869b"),
                cat("alert", "Alerts", "#fb4934"),
            ]),
            day_start_hour: 8,
            day_end_hour: 19,
            life_buffer_min: 15,
            obsidian_vault: String::new(),
            life_calendars: vec![],
            ai: AiSettings {
                allowed_providers: vec!["bedrock".into()],
                region: "us-east-1".into(),
                aws_profile: String::new(),
                // US inference profiles only; the org's BAA covers Bedrock in US regions.
                // Suggestions are simple picks: low effort keeps them fast and cheap.
                small: AiTier { provider: "bedrock".into(), model: "us.openai.gpt-6-luna".into(), effort: "low".into() },
                large: AiTier { provider: "bedrock".into(), model: "us.anthropic.claude-opus-5-5".into(), effort: String::new() },
                prices: BTreeMap::new(),
            },
            fake_now: String::new(),
            ui_scale: 1.0,
            quiet_days: 7,
            sources: vec![],
            demo_data: true,
        }
    }
}

impl Settings {
    /// Load from `path`, writing defaults there first if the file doesn't exist.
    pub fn load_or_init(path: &Path) -> anyhow::Result<Settings> {
        if !path.exists() {
            let s = Settings::default();
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, serde_json::to_string_pretty(&s)?)?;
            return Ok(s);
        }
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

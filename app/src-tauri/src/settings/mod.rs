//! `settings.json`: sources, categories, buffers, AI allow-list.
//!
//! Edited by hand or from the Settings modal; either way the app re-reads the
//! file and applies the change live (`live`). Doc comments on the fields are
//! the Settings page's descriptions: the first line is what the page shows.
//! `x-section`/`x-group`/`x-label` place each field on the page (`schema`).

pub mod api;
pub mod live;
pub mod schema;
pub mod sources;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Category {
    pub key: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacyCategory {
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
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

impl JsonSchema for Categories {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Categories".into()
    }
    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <Vec<Category>>::json_schema(g)
    }
}

/// One configured source. `kind` picks the connector; `name` must be unique per
/// kind. Secrets are NOT stored here: the connector's config names keys in the
/// Keychain (or `secrets.json` on Linux).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SourceConfig {
    /// slack, gmail, gcal, jira, github, obsidian, protoncal or glean.
    pub kind: String,
    /// Personal sources only produce busy blocks.
    #[schemars(extend("x-label" = "Tag", "x-group" = "identity", "x-enum" = ["work", "personal"]))]
    pub tag: String,
    /// Shown in Incoming and the sync status.
    #[schemars(extend("x-label" = "Name", "x-group" = "identity"))]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Seconds between catch-ups. Ignored by live sources.
    #[serde(default = "default_poll")]
    #[schemars(extend("x-label" = "Poll every", "x-group" = "limits", "x-unit" = "s", "x-step" = 30, "x-min" = 30))]
    pub poll_secs: u64,
    /// Connector-specific settings; typed per kind in `sources`.
    #[serde(default)]
    pub config: serde_json::Value,
}

fn yes() -> bool {
    true
}

fn default_poll() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AiTier {
    pub provider: String,
    /// Empty turns this tier off.
    pub model: String,
    /// Reasoning effort: "low", "medium", "high", or "" for the model's default.
    #[serde(default)]
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AiSettings {
    /// Work data only goes to these. Anything else fails closed.
    #[schemars(extend("x-label" = "Allowed providers", "x-group" = "Allowed providers", "x-readonly" = true))]
    pub allowed_providers: Vec<String>,
    /// US regions only under the BAA.
    #[schemars(extend("x-label" = "Region", "x-group" = "Bedrock"))]
    pub region: String,
    /// Empty uses AWS_PROFILE, then default.
    ///
    /// Named profile in `~/.aws/credentials` / `~/.aws/config`. A setting
    /// because apps started from the Dock don't see shell env vars.
    #[serde(default)]
    #[schemars(extend("x-label" = "AWS profile", "x-group" = "Bedrock"))]
    pub aws_profile: String,
    #[schemars(extend("x-label" = "Small", "x-group" = "Models", "x-widget" = "tier"))]
    pub small: AiTier,
    #[schemars(extend("x-label" = "Large", "x-group" = "Models", "x-widget" = "tier"))]
    pub large: AiTier,
    /// Per million tokens, USD. Only used for cost estimates.
    #[serde(default)]
    #[schemars(extend("x-label" = "Prices", "x-group" = "Prices", "x-widget" = "prices"))]
    pub prices: BTreeMap<String, Price>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Topic categories in UI order. Topics and calendar blocks reference keys, which never change.
    #[schemars(extend("x-section" = "categories", "x-widget" = "categories"))]
    pub categories: Categories,
    /// First hour shown on the day strip.
    #[schemars(extend("x-section" = "general", "x-group" = "Day strip", "x-label" = "Day range", "x-widget" = "hour", "x-pair" = "day_end_hour", "x-min" = 0, "x-max" = 23))]
    pub day_start_hour: u32,
    /// Last hour shown on the day strip.
    #[schemars(extend("x-section" = "general", "x-group" = "Day strip", "x-hidden" = true, "x-min" = 1, "x-max" = 24))]
    pub day_end_hour: u32,
    /// Before each Life block. b on a block changes just that one.
    #[schemars(extend("x-section" = "general", "x-group" = "Day strip", "x-label" = "Travel buffer", "x-unit" = "min", "x-step" = 5, "x-min" = 0, "x-max" = 120))]
    pub life_buffer_min: i64,
    /// Empty uses the built-in stand-in.
    ///
    /// Absolute path to the work Obsidian vault. Empty = `<data dir>/vault`.
    #[schemars(extend("x-section" = "general", "x-group" = "Notes", "x-label" = "Work vault", "x-widget" = "folder", "x-restart" = true))]
    pub obsidian_vault: String,
    /// Proton Calendar "Limited view" share links. Moved into the protoncal source's `urls` at startup.
    #[schemars(extend("x-hidden" = true))]
    pub life_calendars: Vec<String>,
    #[schemars(extend("x-section" = "ai"))]
    pub ai: AiSettings,
    /// Pin the clock to HH:MM. Empty uses the real clock.
    #[schemars(extend("x-section" = "developer", "x-group" = "Clock", "x-label" = "Fake now", "x-dev" = true, "x-restart" = true))]
    pub fake_now: String,
    /// Type and spacing grow together.
    #[schemars(extend("x-section" = "general", "x-group" = "Window", "x-label" = "Interface scale", "x-widget" = "scale", "x-step" = 0.05, "x-min" = 0.75, "x-max" = 2.0))]
    pub ui_scale: f64,
    /// Lists, labels and controls. Empty uses the system font.
    #[schemars(extend("x-section" = "general", "x-group" = "Type", "x-label" = "Interface font", "x-widget" = "font"))]
    pub ui_font: String,
    /// Keys, paths, code. Empty uses the system monospace font.
    #[schemars(extend("x-section" = "general", "x-group" = "Type", "x-label" = "Monospace font", "x-widget" = "font", "x-mono" = true))]
    pub mono_font: String,
    /// Lists, labels, controls.
    #[schemars(extend("x-section" = "general", "x-group" = "Type", "x-label" = "Interface size", "x-unit" = "px", "x-step" = 0.5, "x-min" = 10, "x-max" = 20))]
    pub ui_font_size: f64,
    /// Messages, notes, drafts.
    #[schemars(extend("x-section" = "general", "x-group" = "Type", "x-label" = "Reading size", "x-unit" = "px", "x-step" = 0.5, "x-min" = 10, "x-max" = 24))]
    pub reading_font_size: f64,
    /// No new signal for this long moves a topic under Quiet.
    #[schemars(extend("x-section" = "general", "x-group" = "Topics", "x-label" = "Quiet after", "x-unit" = "days", "x-min" = 1, "x-max" = 90))]
    pub quiet_days: i64,
    /// Real sources to sync. Empty while running on sample data.
    #[schemars(extend("x-section" = "sources", "x-widget" = "sources"))]
    pub sources: Vec<SourceConfig>,
    /// Turn off before connecting real sources.
    ///
    /// Fill an empty database with the sample data from the design mocks.
    #[schemars(extend("x-section" = "general", "x-group" = "Data", "x-label" = "Sample data"))]
    pub demo_data: bool,
    /// People notes, person colours and new-person suggestions.
    #[schemars(extend("x-section" = "people"))]
    pub people: PeopleSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct PeopleSettings {
    /// Your colour, as an index into the person palette.
    #[schemars(extend("x-group" = "You", "x-label" = "Your colour", "x-widget" = "person_color", "x-min" = 0, "x-max" = 255))]
    pub my_color: u8,
    /// Your own accounts as kind:account (email:me@work.com).
    ///
    /// Your Slack user id comes from the Slack source itself.
    #[schemars(extend("x-group" = "You", "x-label" = "Your accounts", "x-widget" = "list"))]
    pub me: Vec<String>,
    /// Suggest a People note for a sender seen in this many signals…
    #[schemars(extend("x-group" = "New person suggestions", "x-label" = "Suggest after", "x-unit" = "signals", "x-min" = 1, "x-max" = 50))]
    pub suggest_after: i64,
    /// …within this many days.
    #[schemars(extend("x-group" = "New person suggestions", "x-label" = "Within", "x-unit" = "days", "x-min" = 1, "x-max" = 90))]
    pub suggest_days: i64,
}

impl Default for PeopleSettings {
    fn default() -> Self {
        PeopleSettings { my_color: 0, me: vec![], suggest_after: 3, suggest_days: 7 }
    }
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
            ui_font: String::new(),
            mono_font: String::new(),
            ui_font_size: 13.0,
            reading_font_size: 14.0,
            quiet_days: 7,
            sources: vec![],
            demo_data: true,
            people: PeopleSettings::default(),
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
        live::migrate_file(path)?;
        let text = std::fs::read_to_string(path)?;
        // Not validated here: a bad hand edit mustn't stop the app opening.
        // `live::startup` checks it and shows the problem in Settings.
        Ok(serde_json::from_str(&text)?)
    }

    /// Checks serde can't express: tags, unique source names, typed source configs, ranges.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.day_start_hour >= self.day_end_hour || self.day_end_hour > 24 {
            anyhow::bail!("day_start_hour must be before day_end_hour (0–24)");
        }
        if !(0.5..=3.0).contains(&self.ui_scale) {
            anyhow::bail!("ui_scale must be between 0.5 and 3");
        }
        if !(8.0..=32.0).contains(&self.ui_font_size) || !(8.0..=32.0).contains(&self.reading_font_size) {
            anyhow::bail!("ui_font_size and reading_font_size must be between 8 and 32");
        }
        let mut seen = std::collections::BTreeSet::new();
        for (i, s) in self.sources.iter().enumerate() {
            if s.tag != "work" && s.tag != "personal" {
                anyhow::bail!("sources[{i}] ({}): tag must be work or personal", s.name);
            }
            if s.name.trim().is_empty() {
                anyhow::bail!("sources[{i}]: name can't be empty");
            }
            if !seen.insert((s.kind.clone(), s.name.clone())) {
                anyhow::bail!("sources[{i}]: there's already a {} source named '{}'", s.kind, s.name);
            }
            sources::validate(&s.kind, &s.config).map_err(|e| anyhow::anyhow!("sources[{i}] ({}): {e}", s.name))?;
        }
        Ok(())
    }
}

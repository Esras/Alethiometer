//! Typed `sources[].config`, one struct per connector kind.
//!
//! These are the schema the Settings page renders a source's page from, and
//! what hand edits are validated against. Connectors still read their config
//! from the JSON themselves; `tests::defaults_match_connectors` keeps the
//! defaults here and there in step. Unknown keys are allowed (an older or newer
//! build may know more), wrong types are not.
//!
//! Groups (`x-group`): identity, secrets, reads ("What it reads"), categories, limits.
//! A field with `x-secret` holds the *name* of a secret; the page shows that
//! secret (set or not, when) with Replace…, never its value.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Every connector kind, in the order "Add source" lists them.
pub const KINDS: &[(&str, &str)] = &[
    ("slack", "Slack"),
    ("gmail", "Gmail"),
    ("gcal", "Google Calendar"),
    ("jira", "Jira"),
    ("github", "GitHub"),
    ("obsidian", "Obsidian vault"),
    ("protoncal", "Proton Calendar · personal"),
    ("glean", "Glean"),
];

/// `channels` is a list, or "all".
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum Channels {
    All(AllChannels),
    Only(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AllChannels {
    All,
}

impl Default for Channels {
    fn default() -> Self {
        Channels::Only(vec![])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct SlackConfig {
    #[schemars(extend("x-group" = "secrets", "x-label" = "User token", "x-secret" = true, "x-note" = "xoxp-…"))]
    pub user_token_secret: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "App token", "x-secret" = true, "x-note" = "xapp-… · live feed"))]
    pub app_token_secret: String,
    /// Past ~100, a catch-up takes minutes without the live token.
    #[schemars(extend("x-group" = "reads", "x-label" = "Channels", "x-widget" = "slack_channels"))]
    pub channels: Channels,
    /// Still read, never new in Incoming.
    #[schemars(extend("x-group" = "reads", "x-label" = "Muted", "x-widget" = "list"))]
    pub muted: Vec<String>,
    #[schemars(extend("x-group" = "reads", "x-label" = "DMs"))]
    pub dms: bool,
    /// Needs search:read.
    #[schemars(extend("x-group" = "reads", "x-label" = "Mentions elsewhere"))]
    pub mentions: bool,
    #[schemars(extend("x-group" = "limits", "x-label" = "Look back", "x-unit" = "h", "x-step" = 12, "x-min" = 1))]
    pub lookback_hours: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "DMs per catch-up", "x-step" = 10, "x-min" = 1))]
    pub max_dms: i64,
    /// After the last reply.
    #[schemars(extend("x-group" = "limits", "x-label" = "Track threads for", "x-unit" = "days", "x-step" = 10, "x-min" = 1))]
    pub thread_days: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Threads tracked", "x-step" = 50, "x-min" = 1))]
    pub max_threads: i64,
    /// A full catch-up this often even while live.
    #[schemars(extend("x-group" = "limits", "x-label" = "Safety catch-up", "x-unit" = "h", "x-min" = 1))]
    pub catchup_hours: i64,
}

impl Default for SlackConfig {
    fn default() -> Self {
        SlackConfig {
            user_token_secret: crate::sync::connectors::slack::USER_TOKEN.into(),
            app_token_secret: crate::sync::connectors::slack::APP_TOKEN.into(),
            channels: Channels::default(),
            muted: vec![],
            dms: true,
            mentions: true,
            lookback_hours: 72,
            max_dms: 50,
            thread_days: 90,
            max_threads: 300,
            catchup_hours: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct GmailConfig {
    #[schemars(extend("x-group" = "identity", "x-label" = "Account"))]
    pub account: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "Token name prefix", "x-advanced" = true))]
    pub secret_prefix: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "OAuth client prefix", "x-advanced" = true))]
    pub client_prefix: String,
    /// Category → Gmail label added when you file a thread.
    #[schemars(extend("x-group" = "reads", "x-label" = "Category labels", "x-widget" = "category_map"))]
    pub category_labels: BTreeMap<String, String>,
    #[schemars(extend("x-group" = "reads", "x-label" = "Read only"))]
    pub read_only: bool,
}

impl Default for GmailConfig {
    fn default() -> Self {
        GmailConfig { account: String::new(), secret_prefix: "gmail".into(), client_prefix: "gcal".into(), category_labels: BTreeMap::new(), read_only: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct GcalConfig {
    #[schemars(extend("x-group" = "identity", "x-label" = "Account"))]
    pub account: String,
    #[schemars(extend("x-group" = "identity", "x-label" = "Calendar"))]
    pub calendar_id: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "Secret name prefix", "x-advanced" = true))]
    pub secret_prefix: String,
    #[schemars(extend("x-group" = "categories", "x-label" = "Default category", "x-widget" = "category"))]
    pub default_category: String,
    /// Google colorId → category, for self-holds.
    #[schemars(extend("x-group" = "categories", "x-label" = "Category by colour", "x-widget" = "category_map"))]
    pub category_by_color: BTreeMap<String, String>,
    #[schemars(extend("x-group" = "limits", "x-label" = "Days back", "x-unit" = "days", "x-min" = 0))]
    pub days_back: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Days ahead", "x-unit" = "days", "x-min" = 1))]
    pub days_ahead: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Hold length", "x-unit" = "min", "x-step" = 5, "x-min" = 5, "x-max" = 480))]
    pub hold_minutes: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Hold hours", "x-widget" = "hour_range"))]
    pub hold_hours: Vec<u32>,
    #[schemars(extend("x-group" = "limits", "x-label" = "Hold search", "x-unit" = "workdays", "x-min" = 1, "x-max" = 20))]
    pub hold_search_days: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "RSVP updates", "x-enum" = ["", "all", "externalOnly", "none"]))]
    pub rsvp_send_updates: String,
}

impl Default for GcalConfig {
    fn default() -> Self {
        GcalConfig {
            account: String::new(),
            calendar_id: "primary".into(),
            secret_prefix: "gcal".into(),
            default_category: "work".into(),
            category_by_color: BTreeMap::new(),
            days_back: 30,
            days_ahead: 60,
            hold_minutes: 30,
            hold_hours: vec![9, 17],
            hold_search_days: 5,
            rsvp_send_updates: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct JiraConfig {
    /// https://<site>.atlassian.net
    #[schemars(extend("x-group" = "identity", "x-label" = "Site"))]
    pub base_url: String,
    /// The account that owns the API token.
    #[schemars(extend("x-group" = "identity", "x-label" = "Email"))]
    pub email: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "API token", "x-secret" = true))]
    pub secret: String,
    #[schemars(extend("x-group" = "reads", "x-label" = "JQL", "x-widget" = "code"))]
    pub jql: String,
    /// Custom date-time field holding the real deadline.
    #[schemars(extend("x-group" = "reads", "x-label" = "Due field"))]
    pub due_field: String,
    /// Where Notes → Jira creates issues.
    #[schemars(extend("x-group" = "reads", "x-label" = "Create in project"))]
    pub create_project: String,
    #[schemars(extend("x-group" = "reads", "x-label" = "Issue type"))]
    pub create_issue_type: String,
    #[schemars(extend("x-group" = "limits", "x-label" = "Look back", "x-unit" = "days", "x-min" = 1))]
    pub lookback_days: i64,
    /// 0 turns the duplicate check off.
    #[schemars(extend("x-group" = "limits", "x-label" = "Duplicate check", "x-unit" = "h", "x-min" = 0))]
    pub create_dedupe_hours: i64,
}

impl Default for JiraConfig {
    fn default() -> Self {
        JiraConfig {
            base_url: String::new(),
            email: String::new(),
            secret: crate::sync::connectors::jira::DEFAULT_SECRET.into(),
            jql: crate::sync::connectors::jira::DEFAULT_JQL.into(),
            due_field: String::new(),
            create_project: String::new(),
            create_issue_type: "Task".into(),
            lookback_days: 14,
            create_dedupe_hours: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct GithubConfig {
    /// Empty uses api.github.com. Set for GitHub Enterprise.
    #[schemars(extend("x-group" = "identity", "x-label" = "API URL"))]
    pub api_url: String,
    /// Without a stored token: the gh account to use.
    #[schemars(extend("x-group" = "identity", "x-label" = "gh account"))]
    pub gh_user: String,
    #[schemars(extend("x-group" = "identity", "x-label" = "gh path", "x-advanced" = true))]
    pub gh_path: String,
    /// Empty takes the token from gh.
    #[schemars(extend("x-group" = "secrets", "x-label" = "Token", "x-secret" = true, "x-optional" = true))]
    pub secret: String,
    /// owner/repo; takes precedence over orgs.
    #[schemars(extend("x-group" = "reads", "x-label" = "Repos", "x-widget" = "list"))]
    pub repos: Vec<String>,
    #[schemars(extend("x-group" = "reads", "x-label" = "Orgs", "x-widget" = "list"))]
    pub orgs: Vec<String>,
    /// Off skips PRs you were only pulled into through a team.
    #[schemars(extend("x-group" = "reads", "x-label" = "Team review requests"))]
    pub team_review_requests: bool,
    #[schemars(extend("x-group" = "reads", "x-label" = "Read only"))]
    pub read_only: bool,
    #[schemars(extend("x-group" = "limits", "x-label" = "Look back", "x-unit" = "days", "x-min" = 1))]
    pub lookback_days: i64,
}

impl Default for GithubConfig {
    fn default() -> Self {
        GithubConfig {
            api_url: String::new(),
            gh_user: String::new(),
            gh_path: String::new(),
            secret: crate::sync::connectors::github::DEFAULT_SECRET.into(),
            repos: vec![],
            orgs: vec![],
            team_review_requests: true,
            read_only: false,
            lookback_days: 14,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct ObsidianConfig {
    /// Empty uses the Work vault from General.
    #[schemars(extend("x-group" = "identity", "x-label" = "Vault", "x-widget" = "folder"))]
    pub vault: String,
    /// obsidian writes through the running app; file writes the files directly.
    #[schemars(extend("x-group" = "reads", "x-label" = "Write through", "x-enum" = ["obsidian", "file"]))]
    pub write_mode: String,
    #[schemars(extend("x-group" = "reads", "x-label" = "Ignore", "x-widget" = "list"))]
    pub ignore: Vec<String>,
    #[schemars(extend("x-group" = "limits", "x-label" = "REST URL", "x-advanced" = true))]
    pub rest_url: String,
    /// PEM file (or data.json) with the CA to trust.
    #[schemars(extend("x-group" = "limits", "x-label" = "Certificate", "x-advanced" = true))]
    pub cert_path: String,
}

impl Default for ObsidianConfig {
    fn default() -> Self {
        ObsidianConfig {
            vault: String::new(),
            write_mode: "obsidian".into(),
            ignore: vec![],
            rest_url: crate::sync::connectors::obsidian::rest::DEFAULT_URL.into(),
            cert_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct ProtoncalConfig {
    /// Proton "Limited view" links. Busy times only.
    #[schemars(extend("x-group" = "secrets", "x-label" = "Calendar links", "x-widget" = "masked_list"))]
    pub urls: Vec<String>,
    #[schemars(extend("x-group" = "limits", "x-label" = "Days back", "x-unit" = "days", "x-min" = 0))]
    pub past_days: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Days ahead", "x-unit" = "days", "x-min" = 1))]
    pub future_days: i64,
    #[schemars(extend("x-group" = "reads", "x-label" = "All-day events", "x-enum" = ["busy", "skip"]))]
    pub all_day: String,
}

impl Default for ProtoncalConfig {
    fn default() -> Self {
        ProtoncalConfig { urls: vec![], past_days: 1, future_days: 14, all_day: "busy".into() }
    }
}

/// Glean is search-only: it's never polled and nothing it returns is stored
/// as an item. ⌘K, the Search view and the Assistant query it on demand.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct GleanConfig {
    /// Your Glean API server, e.g. https://acme-be.glean.com
    #[schemars(extend("x-group" = "identity", "x-label" = "Server"))]
    pub server_url: String,
    /// Only for a global token: the work email to search as.
    #[schemars(extend("x-group" = "identity", "x-label" = "Act as", "x-advanced" = true))]
    pub act_as: String,
    #[schemars(extend("x-group" = "secrets", "x-label" = "API token", "x-secret" = true))]
    pub token_secret: String,
    #[schemars(extend("x-group" = "reads", "x-label" = "Datasources", "x-widget" = "list"))]
    pub datasources: Vec<String>,
    /// Never search these (e.g. slack: it's read directly).
    #[schemars(extend("x-group" = "reads", "x-label" = "Skip datasources", "x-widget" = "list"))]
    pub exclude_datasources: Vec<String>,
    #[schemars(extend("x-group" = "reads", "x-label" = "Window", "x-unit" = "days", "x-step" = 30, "x-min" = 7))]
    pub window_days: i64,
    /// Anchored results below this similarity to the topic are hidden.
    #[schemars(extend("x-group" = "reads", "x-label" = "Off-topic below", "x-step" = 0.02, "x-min" = 0.0, "x-max" = 1.0))]
    pub min_similarity: f64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Results per query", "x-step" = 5, "x-min" = 5, "x-max" = 100))]
    pub results_per_query: i64,
    /// Glean queries the Assistant runs per question.
    #[schemars(extend("x-group" = "limits", "x-label" = "Queries per ask", "x-min" = 1, "x-max" = 6))]
    pub queries_per_ask: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Cache", "x-unit" = "min", "x-step" = 5, "x-min" = 0))]
    pub cache_minutes: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Calls per day", "x-step" = 50, "x-min" = 1, "x-widget" = "glean_calls"))]
    pub calls_per_day: i64,
    #[schemars(extend("x-group" = "limits", "x-label" = "Timeout", "x-unit" = "s", "x-min" = 1, "x-max" = 60))]
    pub timeout_secs: i64,
}

impl Default for GleanConfig {
    fn default() -> Self {
        GleanConfig {
            server_url: String::new(),
            act_as: String::new(),
            token_secret: "glean.token".into(),
            datasources: vec![],
            exclude_datasources: vec![],
            window_days: 180,
            min_similarity: 0.62,
            results_per_query: 30,
            queries_per_ask: 3,
            cache_minutes: 15,
            calls_per_day: 400,
            timeout_secs: 8,
        }
    }
}

impl GleanConfig {
    pub fn from_json(v: &Value) -> anyhow::Result<GleanConfig> {
        if v.is_null() {
            return Ok(GleanConfig::default());
        }
        Ok(serde_json::from_value(v.clone())?)
    }
}

/// One secret a source uses, for the Secrets card.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SecretSlot {
    pub name: String,
    pub label: String,
    /// What it is ("xoxp-…", "live feed").
    pub note: String,
    /// The source works without it.
    pub optional: bool,
    /// Written by Connect (OAuth), not pasted.
    pub oauth: bool,
}

/// The secrets a source of `kind` reads, by name, with this config.
pub fn secrets(kind: &str, config: &Value) -> Vec<SecretSlot> {
    let slot = |name: String, label: &str, note: &str, optional: bool, oauth: bool| SecretSlot { name, label: label.into(), note: note.into(), optional, oauth };
    let get = |k: &str, d: &str| config.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(d).to_string();
    match kind {
        "slack" => {
            let d = SlackConfig::default();
            vec![
                slot(get("user_token_secret", &d.user_token_secret), "User token", "xoxp-…", false, false),
                slot(get("app_token_secret", &d.app_token_secret), "App token", "xapp-… · live feed", true, false),
            ]
        }
        "jira" => vec![slot(get("secret", crate::sync::connectors::jira::DEFAULT_SECRET), "API token", "", false, false)],
        "github" => vec![slot(get("secret", crate::sync::connectors::github::DEFAULT_SECRET), "Token", "empty takes it from gh", true, false)],
        "glean" => vec![slot(get("token_secret", "glean.token"), "API token", "Glean API token with search scope", false, false)],
        "obsidian" => vec![slot(crate::sync::connectors::obsidian::rest::API_KEY.into(), "REST API key", "Local REST API plugin", true, false)],
        "gcal" => {
            let p = get("secret_prefix", "gcal");
            vec![
                slot(format!("{p}.client_id"), "OAuth client ID", "…apps.googleusercontent.com", false, false),
                slot(format!("{p}.client_secret"), "OAuth client secret", "", false, false),
                slot(format!("{p}.refresh_token"), "Sign-in", "written by Connect", false, true),
            ]
        }
        "gmail" => {
            let c = get("client_prefix", "gcal");
            let p = get("secret_prefix", "gmail");
            vec![
                slot(format!("{c}.client_id"), "OAuth client ID", "shared with the calendar", false, false),
                slot(format!("{c}.client_secret"), "OAuth client secret", "shared with the calendar", false, false),
                slot(format!("{p}.refresh_token"), "Sign-in", "written by Connect", false, true),
            ]
        }
        _ => vec![],
    }
}

/// Check a source's config against its kind's struct.
pub fn validate(kind: &str, config: &Value) -> anyhow::Result<()> {
    if config.is_null() {
        return Ok(());
    }
    if !config.is_object() {
        anyhow::bail!("config must be an object");
    }
    fn check<T: serde::de::DeserializeOwned>(v: &Value) -> anyhow::Result<()> {
        serde_json::from_value::<T>(v.clone()).map(|_| ()).map_err(|e| anyhow::anyhow!("config: {e}"))
    }
    match kind {
        "slack" => check::<SlackConfig>(config),
        "gmail" => check::<GmailConfig>(config),
        "gcal" => check::<GcalConfig>(config),
        "jira" => check::<JiraConfig>(config),
        "github" => check::<GithubConfig>(config),
        "obsidian" => check::<ObsidianConfig>(config),
        "protoncal" => check::<ProtoncalConfig>(config),
        "glean" => {
            let c: GleanConfig = serde_json::from_value(config.clone()).map_err(|e| anyhow::anyhow!("config: {e}"))?;
            // Plain http only to this machine (a local stand-in during development).
            let loopback = ["http://127.0.0.1", "http://localhost"].iter().any(|p| c.server_url.starts_with(p));
            if !c.server_url.is_empty() && !c.server_url.starts_with("https://") && !loopback {
                anyhow::bail!("config.server_url must start with https://");
            }
            Ok(())
        }
        other => anyhow::bail!("unknown source kind '{other}'"),
    }
}

/// Defaults for a new source of `kind`, as JSON.
pub fn defaults(kind: &str) -> Option<Value> {
    Some(match kind {
        "slack" => serde_json::to_value(SlackConfig::default()),
        "gmail" => serde_json::to_value(GmailConfig::default()),
        "gcal" => serde_json::to_value(GcalConfig::default()),
        "jira" => serde_json::to_value(JiraConfig::default()),
        "github" => serde_json::to_value(GithubConfig::default()),
        "obsidian" => serde_json::to_value(ObsidianConfig::default()),
        "protoncal" => serde_json::to_value(ProtoncalConfig::default()),
        "glean" => serde_json::to_value(GleanConfig::default()),
        _ => return None,
    }
    .ok()?)
}

/// The JSON schema of each kind's config.
pub fn schemas() -> serde_json::Map<String, Value> {
    fn s<T: JsonSchema>() -> Value {
        serde_json::to_value(schemars::schema_for!(T)).unwrap_or(Value::Null)
    }
    let mut m = serde_json::Map::new();
    m.insert("slack".into(), s::<SlackConfig>());
    m.insert("gmail".into(), s::<GmailConfig>());
    m.insert("gcal".into(), s::<GcalConfig>());
    m.insert("jira".into(), s::<JiraConfig>());
    m.insert("github".into(), s::<GithubConfig>());
    m.insert("obsidian".into(), s::<ObsidianConfig>());
    m.insert("protoncal".into(), s::<ProtoncalConfig>());
    m.insert("glean".into(), s::<GleanConfig>());
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_connectors() {
        let slack = crate::sync::connectors::slack::Config::from_json(&json!({}));
        let d = SlackConfig::default();
        assert_eq!(
            (slack.lookback_hours, slack.max_dms as i64, slack.thread_days, slack.max_threads as i64, slack.catchup_hours, slack.dms, slack.mentions),
            (d.lookback_hours, d.max_dms, d.thread_days, d.max_threads, d.catchup_hours, d.dms, d.mentions)
        );
        assert_eq!((slack.user_token_secret, slack.app_token_secret), (d.user_token_secret, d.app_token_secret));
        let g = crate::sync::connectors::gcal::Config::from(&json!({}));
        let d = GcalConfig::default();
        assert_eq!(
            (g.calendar_id, g.days_back, g.days_ahead, g.hold_minutes, g.hold_day_start, g.hold_day_end, g.hold_search_days as i64, g.default_category),
            (d.calendar_id, d.days_back, d.days_ahead, d.hold_minutes, d.hold_hours[0], d.hold_hours[1], d.hold_search_days, d.default_category)
        );
    }

    #[test]
    fn the_real_configs_validate() {
        // Shapes from docs/sources and the work Mac's settings.json.
        validate("slack", &json!({ "channels": "all", "lookback_hours": 12, "muted": ["#random"] })).unwrap();
        validate("slack", &json!({ "channels": ["#inc-2291", "C0123"] })).unwrap();
        validate("gmail", &json!({ "account": "me@work.com", "category_labels": { "work": "Work" } })).unwrap();
        validate("gcal", &json!({ "category_by_color": { "3": "self" }, "hold_hours": [9, 17] })).unwrap();
        validate("jira", &json!({ "base_url": "https://x.atlassian.net", "email": "a@b", "jql": "project = X" })).unwrap();
        validate("github", &json!({ "team_review_requests": false, "read_only": true })).unwrap();
        validate("obsidian", &json!({ "vault": "/v", "write_mode": "file" })).unwrap();
        validate("protoncal", &json!({ "urls": ["https://calendar.proton.me/x"] })).unwrap();
        validate("glean", &json!({ "server_url": "https://acme-be.glean.com" })).unwrap();
        validate("slack", &json!({ "someday_key": 1 })).expect("unknown keys are allowed");
        let e = validate("slack", &json!({ "lookback_hours": "lots" })).unwrap_err().to_string();
        assert!(e.contains("lookback_hours") || e.contains("invalid type"), "{e}");
        assert!(validate("glean", &json!({ "server_url": "http://x" })).is_err());
        assert!(validate("fax", &json!({})).is_err());
    }
}

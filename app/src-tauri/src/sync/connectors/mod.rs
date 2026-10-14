//! One module per source kind.

pub mod agents;
pub mod chat;
pub mod gcal;
pub mod github;
pub mod gmail;
pub mod jira;
pub mod obsidian;
pub mod protoncal;
pub mod scion;
pub mod slack;

use super::Connector;

pub fn for_kind(kind: &str) -> Option<Box<dyn Connector>> {
    Some(match kind {
        "gcal" => Box::new(gcal::GoogleCalendar),
        "gmail" => Box::new(gmail::Gmail),
        "slack" => Box::new(slack::Slack),
        "jira" => Box::new(jira::Jira),
        "github" => Box::new(github::GitHub),
        "obsidian" => Box::new(obsidian::Obsidian),
        "protoncal" => Box::new(protoncal::ProtonCalendar),
        "scion" => Box::new(scion::Scion),
        "agents" => Box::new(agents::Agents),
        _ => return None,
    })
}

/// A web link as a link into a chat source's desktop app, when one knows it
/// (`Connector::deep_link`).
pub fn deep_link(r: &dyn crate::domain::ports::Reads, url: &str) -> Option<String> {
    chat::CHAT_KINDS
        .iter()
        .filter_map(|k| for_kind(k))
        .find_map(|c| c.deep_link(r, url))
}

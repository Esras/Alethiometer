//! One module per source kind.

pub mod github;
pub mod gcal;
pub mod gmail;
pub mod jira;
pub mod obsidian;
pub mod protoncal;
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
        _ => return None,
    })
}

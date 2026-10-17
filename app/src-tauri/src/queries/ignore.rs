//! Ignore suggestions: what you keep dismissing in Incoming, by sender, list,
//! channel, repo or project, and how to stop seeing it.
//!
//! - Every dismiss, Gmail trash, confirm and move is recorded (`record`) with
//!   what it was about (`Subject`); `u` forgets it.
//! - After a dismiss or trash, the broadest of the signal's keys that passes
//!   `settings.ignore_suggestions` is suggested under the row (`suggest`).
//!   A domain also needs two different senders dismissed. Direct
//!   conversations (DMs, group DMs, 1:1 mail) are never suggested.
//! - Suggest only (user, 2026-10-17): Gmail gives the filter query to copy,
//!   Slack and Teams the entry for the source's `muted` list (the UI adds it
//!   through `settings.mute`), GitHub and Jira a link to the repo or project.
//!   "Not now" holds a key back for `NOT_NOW_DAYS`.

use crate::domain::items::ItemRef;
use crate::domain::ports::UnitOfWork;
use crate::domain::triage::TriageKey;
use crate::error::Result;
use crate::settings::IgnoreSuggestions;
use crate::time::Ts;
use jiff::SignedDuration;
use serde::Serialize;
use serde_json::Value;

pub const NOT_NOW_DAYS: i64 = 30;

/// What a triage action on an item counts toward.
#[derive(Debug, Clone, PartialEq)]
pub struct Subject {
    /// Broadest first, each with how a suggestion shows it and what it offers.
    pub keys: Vec<(TriageKey, String, Offer)>,
    /// A DM, group DM or 1:1 mail: recorded, never suggested.
    pub direct: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Offer {
    /// A Gmail search to paste into a filter.
    Copy { text: String },
    /// An entry for the source's `muted` list.
    Mute { entry: String },
    /// Where to unwatch it.
    Open { url: String },
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub source_id: i64,
    /// The source's kind and name, which `settings.mute` finds it by.
    pub source_kind: String,
    pub source_name: String,
    pub kind: String,
    pub key: String,
    pub label: String,
    pub dismissed: i64,
    pub total: i64,
    pub days: i64,
    pub offer: Offer,
}

fn key(kind: &str, key: impl Into<String>) -> TriageKey {
    TriageKey {
        kind: kind.into(),
        key: key.into(),
    }
}

fn s<'a>(raw: &'a Value, k: &str) -> Option<&'a str> {
    raw.get(k).and_then(Value::as_str).filter(|v| !v.is_empty())
}

/// The keys an item's triage counts toward. None for sources this doesn't
/// cover, and for items synced before their connector recorded the fields.
pub fn subject(item: &ItemRef) -> Option<Subject> {
    let raw: Value = serde_json::from_str(&item.raw_json).unwrap_or_default();
    let (keys, direct) = match item.source_kind.as_str() {
        "gmail" => {
            let mut keys = vec![];
            let from = s(&raw, "from");
            if let Some(domain) = from.and_then(|f| f.rsplit_once('@')).map(|(_, d)| d) {
                let label = format!("@{domain}");
                let text = format!("from:({label})");
                keys.push((key("domain", domain), label, Offer::Copy { text }));
            }
            if let Some(list) = s(&raw, "list") {
                let text = format!("list:({list})");
                keys.push((key("list", list), format!("List {list}"), Offer::Copy { text }));
            }
            if let Some(from) = from {
                let text = format!("from:({from})");
                keys.push((key("from", from), from.to_owned(), Offer::Copy { text }));
            }
            // Older items have no `direct`: they have no `from` either, so no keys.
            (keys, raw.get("direct").and_then(Value::as_bool).unwrap_or(true))
        }
        "slack" => {
            let channel = s(&raw, "channel")?;
            let conv = s(&raw, "conv").unwrap_or_else(|| {
                if item.title.starts_with("DM ·") || channel.starts_with('D') {
                    "dm"
                } else if item.title.starts_with("Group DM") {
                    "group"
                } else {
                    "channel"
                }
            });
            let (label, entry) = match s(&raw, "name") {
                Some(n) => (format!("#{n}"), format!("#{n}")),
                // Older items: the name off a channel's title ("#inc-2291 · mention"), the id to mute.
                None => {
                    let titled = item.title.split(" · ").next().filter(|t| t.starts_with('#'));
                    (titled.unwrap_or(channel).to_owned(), channel.to_owned())
                }
            };
            (
                vec![(key("channel", channel), label, Offer::Mute { entry })],
                conv != "channel",
            )
        }
        "teams" => {
            let channel = s(&raw, "channel")?;
            let label = s(&raw, "name").unwrap_or(&item.title).to_owned();
            let entry = channel.to_owned();
            // Older items without `conv` count as direct: they can't be told apart.
            let direct = s(&raw, "conv") != Some("channel");
            (vec![(key("channel", channel), label, Offer::Mute { entry })], direct)
        }
        "github" => {
            let (repo, _) = item.external_id.rsplit_once('#')?;
            let url = format!("https://github.com/{repo}");
            (vec![(key("repo", repo), repo.to_owned(), Offer::Open { url })], false)
        }
        "jira" => {
            let project = raw
                .pointer("/fields/project/key")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| item.external_id.rsplit_once('-').map(|(p, _)| p.to_owned()))?;
            let base = item
                .url
                .as_deref()
                .and_then(|u| u.split_once("/browse/"))
                .map(|(b, _)| b)?;
            let url = format!("{base}/browse/{project}");
            (vec![(key("project", &project), project, Offer::Open { url })], false)
        }
        _ => return None,
    };
    (!keys.is_empty()).then_some(Subject { keys, direct })
}

/// Record a triage action (`dismiss`, `trash` or `keep`) on the item. The event's id, for `u`.
pub fn record(uow: &dyn UnitOfWork, now: Ts, item_id: i64, action: &str) -> Result<Option<i64>> {
    let Some(item) = uow.items().get(item_id)? else {
        return Ok(None);
    };
    let Some(sub) = subject(&item) else {
        return Ok(None);
    };
    let keys: Vec<TriageKey> = sub.keys.into_iter().map(|(k, ..)| k).collect();
    Ok(Some(uow.triage().record(
        item_id,
        item.source_id,
        action,
        now,
        &keys,
    )?))
}

/// `u`: forget what `record` wrote.
pub fn forget(uow: &dyn UnitOfWork, event: Option<i64>, item_id: i64) -> Result<()> {
    match event {
        Some(e) => uow.triage().forget(e, item_id),
        None => Ok(()),
    }
}

/// The suggestion to show after dismissing the item, if any.
pub fn suggest(uow: &dyn UnitOfWork, now: Ts, cfg: &IgnoreSuggestions, item_id: i64) -> Result<Option<Suggestion>> {
    let r = uow.reads();
    let Some(item) = uow.items().get(item_id)? else {
        return Ok(None);
    };
    let Some(sub) = subject(&item).filter(|s| !s.direct) else {
        return Ok(None);
    };
    let days = cfg.window_days.max(1);
    let since = now - SignedDuration::from_hours(24 * days);
    for (k, label, offer) in sub.keys {
        let st = r.triage().key_stats(item.source_id, &k, since)?;
        let passes = st.total >= cfg.min_actions.max(1)
            && st.dismissed * 100 >= cfg.min_share * st.total
            && (k.kind != "domain" || st.senders >= 2);
        if !passes || r.triage().declined_until(item.source_id, &k)?.is_some_and(|u| u > now) {
            continue;
        }
        let Some(src) = r.sources().get(item.source_id)? else {
            return Ok(None);
        };
        return Ok(Some(Suggestion {
            source_id: item.source_id,
            source_kind: src.kind,
            source_name: src.name,
            kind: k.kind,
            key: k.key,
            label,
            dismissed: st.dismissed,
            total: st.total,
            days,
            offer,
        }));
    }
    Ok(None)
}

/// "Not now" on a suggestion.
pub fn not_now(uow: &dyn UnitOfWork, now: Ts, source_id: i64, k: &TriageKey) -> Result<()> {
    uow.triage()
        .decline(source_id, k, now + SignedDuration::from_hours(24 * NOT_NOW_DAYS))
}

#[cfg(test)]
mod tests;

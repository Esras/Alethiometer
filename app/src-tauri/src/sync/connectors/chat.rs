//! What chat connectors (Slack, Teams) share; the model is in
//! `app/docs/chat.md`. A stream (a conversation, or a thread in one) is read
//! as runs: its messages since the last run you triaged form one item, which
//! new messages join until you triage it. The cursor keeps the runs still
//! open (`open`) and, per stream, where the last triaged one ended
//! (`closed`). Before each poll the runs you triaged here are moved from one
//! to the other, so the next message starts a new run.

use crate::domain::ports::Reads;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use crate::settings::sources::{is_chat, CHAT_KINDS};

/// A run not triaged yet.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Open {
    /// The item's external id.
    pub id: String,
    /// First and last message key of the stored run.
    pub first: String,
    pub last: String,
}

/// A chat connector's cursor: its runs, keyed by stream ("<conversation>",
/// "<conversation>/<thread root key>") or, for a mention, the item's
/// external id.
pub trait Runs: Default + Serialize + DeserializeOwned {
    fn runs(&mut self) -> (&mut BTreeMap<String, Open>, &mut BTreeMap<String, String>);
}

/// The cursor with the runs you triaged in Alethiometer closed.
pub fn close_triaged<S: Runs>(r: &dyn Reads, source: &crate::sync::Source) -> crate::error::Result<Option<String>> {
    let Some(cursor) = source.cursor.as_deref() else {
        return Ok(None);
    };
    let mut state: S = serde_json::from_str(cursor).unwrap_or_default();
    let (open, closed) = state.runs();
    let mut done = vec![];
    for (key, o) in open.iter() {
        let mut triaged = r.items().seen_by_external(source.id, &o.id)?;
        // A thread attached to its parent closes with the item holding the parent.
        if let (None, Some((channel, parent))) = (triaged, key.split_once('/')) {
            if let Some(h) = r.items().holder(source.id, Some(channel), parent)? {
                triaged = r.items().seen(h)?;
            }
        }
        if triaged == Some(true) {
            done.push((key.clone(), o.last.clone()));
        }
    }
    if done.is_empty() {
        return Ok(Some(cursor.to_owned()));
    }
    for (key, last) in done {
        open.remove(&key);
        closed.insert(key, last);
    }
    Ok(Some(serde_json::to_string(&state)?))
}

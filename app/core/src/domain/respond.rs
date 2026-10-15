//! Answering from Alethiometer: calendar invites (RSVP) and Slack replies.

use crate::error::Result;
use crate::time::Ts;

/// A calendar item you're a guest of.
#[derive(Debug, Clone)]
pub struct InviteRow {
    pub item_id: i64,
    /// Your `responseStatus` in Google (`needsAction`, `accepted`, …).
    pub response: String,
    /// What an RSVP answers: the series for a recurring event, else the event.
    pub event_id: String,
    pub start: Ts,
    pub end: Option<Ts>,
    /// The event in Google Calendar, where an answer can be changed.
    pub url: Option<String>,
}

/// Another block on the day strip that overlaps an invite.
#[derive(Debug, Clone)]
pub struct OverlapRow {
    pub item_id: i64,
    /// `None` for a Life block, which never has a title.
    pub title: Option<String>,
    pub start: Ts,
    pub end: Ts,
}

/// The latest outbox row of an answer.
#[derive(Debug, Clone)]
pub struct AnswerRow {
    /// The item it answers (RSVPs: every item of the event or series it answered).
    pub item_id: i64,
    pub outbox_id: i64,
    /// pending | sent | confirmed | failed
    pub state: String,
    pub payload_json: String,
    pub error: Option<String>,
    pub created_at: Ts,
    pub confirmed_at: Option<Ts>,
}

pub trait RespondViews {
    /// The items among these that are calendar events with you on the guest list.
    fn invites(&self, item_ids: &[i64]) -> Result<Vec<InviteRow>>;
    /// Day strip blocks (Life included) that overlap these items' own blocks, by start.
    fn overlaps(&self, item_ids: &[i64]) -> Result<Vec<OverlapRow>>;
    /// The latest `gcal.rsvp` for each of these items' events (or series).
    fn latest_rsvps(&self, item_ids: &[i64]) -> Result<Vec<AnswerRow>>;
    /// The latest chat reply (`<source kind>.reply`) queued from each of these items.
    fn latest_replies(&self, item_ids: &[i64]) -> Result<Vec<AnswerRow>>;
}

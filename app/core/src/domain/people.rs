//! People: People notes mirrored into the database, the accounts they list,
//! and what sources say about accounts (profiles, bots).

use crate::error::Result;
use crate::time::Ts;

/// A person from a People note.
#[derive(Debug, Clone)]
pub struct PersonRow {
    pub id: i64,
    pub name: String,
    pub note_path: String,
    pub role: Option<String>,
    /// Set in the note; otherwise derived from the person's accounts.
    pub color: Option<u8>,
    pub aliases: Vec<String>,
    /// How much a todo this person asked for matters, 0 to 1 (the note's `weight`).
    pub weight: Option<f64>,
}

/// An account a People note lists.
#[derive(Debug, Clone)]
pub struct AccountRow {
    pub kind: String,
    pub account: String,
    pub person_id: i64,
}

/// What a source's profile says about an account (Slack).
#[derive(Debug, Clone)]
pub struct Profile {
    pub kind: String,
    pub account: String,
    pub real_name: Option<String>,
    pub email: Option<String>,
}

/// Everything needed to match an author to a person.
#[derive(Debug, Default)]
pub struct Directory {
    pub people: Vec<PersonRow>,
    /// In the order the notes were read.
    pub accounts: Vec<AccountRow>,
    pub profiles: Vec<Profile>,
}

/// An author seen in a work signal: a message's sender or an item's author.
#[derive(Debug, Clone)]
pub struct SeenAuthor {
    pub source_kind: String,
    /// As the source stores it (`author_id`, or the item author's handle).
    pub handle: String,
    pub name: String,
    /// The signal it was in; split parts count as their root.
    pub root: i64,
    pub at: Ts,
}

/// Someone who wrote in a topic's confirmed work signals: a message's sender
/// or the item's author.
#[derive(Debug, Clone)]
pub struct TopicAuthor {
    pub source_kind: String,
    pub handle: Option<String>,
    pub name: String,
    pub item_id: i64,
    pub at: Ts,
}

/// Someone who wrote in a confirmed work signal, with the topic it's
/// confirmed to: one row per message they sent, or for the item they authored.
#[derive(Debug, Clone)]
pub struct SignalAuthor {
    pub source_kind: String,
    pub handle: Option<String>,
    pub name: String,
    pub item_id: i64,
    pub topic_id: i64,
    pub at: Ts,
}

/// An author's latest confirmed work signal in a topic, over all time.
#[derive(Debug, Clone)]
pub struct AuthorTopic {
    pub source_kind: String,
    pub handle: Option<String>,
    pub name: String,
    pub topic_id: i64,
    pub last_at: Ts,
}

/// A guest (not you, not a room) on an upcoming timed event of the work calendar.
#[derive(Debug, Clone)]
pub struct Guest {
    /// Lowercased.
    pub email: String,
    /// Their answer as Google gives it (`accepted`, `declined`, `tentative`, `needsAction`).
    pub response: Option<String>,
    pub start: Ts,
    pub title: String,
}

/// A person linked to a topic by hand.
#[derive(Debug, Clone)]
pub struct LinkedPerson {
    pub id: i64,
    pub name: String,
    pub role: Option<String>,
}

pub trait PeopleViews {
    fn directory(&self) -> Result<Directory>;
    /// Accounts whose new-person suggestion is snoozed past `now`.
    fn snoozed(&self, now: Ts) -> Result<Vec<(String, String)>>;
    /// Accounts their source marks as bots.
    fn bots(&self) -> Result<Vec<(String, String)>>;
    /// Authors in work signals since `since`.
    fn authors_since(&self, since: Ts) -> Result<Vec<SeenAuthor>>;
    fn topic_authors(&self, topic_id: i64) -> Result<Vec<TopicAuthor>>;
    fn linked_to_topic(&self, topic_id: i64) -> Result<Vec<LinkedPerson>>;
    /// Open work notes (vault paths) that link to a person's note, other than the note itself.
    fn notes_linking(&self, note_path: &str, name: &str, limit: usize) -> Result<Vec<String>>;
    /// Topics a person's messages were filed under: name, signals, latest message; most recent first.
    fn filed_by(&self, person_id: i64, limit: usize) -> Result<Vec<(String, i64, Ts)>>;
    /// A person's note path and name, when they have a note.
    fn note_of(&self, person_id: i64) -> Result<Option<(String, String)>>;
    /// Authors of confirmed work signals since `since` (by message time, or the item's time for its author).
    fn signal_authors_since(&self, since: Ts) -> Result<Vec<SignalAuthor>>;
    /// Every author of a confirmed work signal, per topic, with their latest time there.
    fn author_topics(&self) -> Result<Vec<AuthorTopic>>;
    /// Guests on the work calendar's timed events starting in (`after`, `until`], soonest first.
    fn upcoming_guests(&self, after: Ts, until: Ts) -> Result<Vec<Guest>>;
}

/// A People note as read from the vault.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonNote {
    /// Vault-relative ("People/Kim Lee.md").
    pub path: String,
    pub name: String,
    pub role: Option<String>,
    pub color: Option<u8>,
    pub aliases: Vec<String>,
    /// `weight`, clamped to 0..=1.
    pub weight: Option<f64>,
    /// (kind, account), normalized.
    pub accounts: Vec<(String, String)>,
}

pub trait People {
    /// Make the people rows match these notes (every note in `People/`, by
    /// path). A person whose note is gone keeps its row (topics may point at
    /// it) but loses its note path and accounts. Two notes claiming one
    /// account: the first keeps it.
    fn mirror(&self, notes: &[PersonNote]) -> Result<()>;
    /// A note write is queued: record the person and account now, so colours
    /// follow; the next scan has the final word.
    fn note_queued(&self, path: &str, name: &str, account: Option<(&str, &str)>) -> Result<()>;
    /// Hide the new-person suggestion for an account until `until`.
    fn snooze(&self, kind: &str, account: &str, until: Ts) -> Result<()>;
    /// A merge of `from`'s note into `into`'s landed: topics linked to `from` by hand link to `into`.
    fn move_topic_links(&self, from: i64, into: i64) -> Result<()>;
}

/// Where People notes live in the vault.
pub const PEOPLE_DIR: &str = "People";

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

pub trait PeopleViews {
    fn directory(&self) -> Result<Directory>;
    /// Accounts whose new-person suggestion is snoozed past `now`.
    fn snoozed(&self, now: Ts) -> Result<Vec<(String, String)>>;
    /// Accounts their source marks as bots.
    fn bots(&self) -> Result<Vec<(String, String)>>;
    /// Authors in work signals since `since`.
    fn authors_since(&self, since: Ts) -> Result<Vec<SeenAuthor>>;
}

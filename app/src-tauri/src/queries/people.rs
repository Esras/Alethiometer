//! People (design 17a new person, 17b link to person, 16c person colours).
//!
//! - A person is a note in the work vault, `People/<Name>.md`. Its frontmatter
//!   lists the person's accounts (`slack`, `email`, `jira`, `github`, each a
//!   value or a list), Obsidian's `aliases`, and optionally `color` (an index
//!   into the 256-colour palette). Other fields (`team`, `manager`, `role`…)
//!   are free-form; only `role` is shown, under the name.
//! - `scan` mirrors the notes into `people` (`note_path` set) and
//!   `person_accounts`. The notes are the source of truth: every scan replaces
//!   the accounts. It runs at startup, after an Obsidian sync touches
//!   `People/`, and after the app writes a People note itself.
//! - Authors are matched at read time (`Resolver`): a message's `author_id`
//!   or an item's author handle, mapped to an account (`account_of`), then
//!   through `person_accounts`. A Slack account whose profile has an email
//!   (`account_profiles`, `users:read.email`) also matches a note by that
//!   email, and is coloured by it. Editing a note in Obsidian recolours past and
//!   future signals alike. Authors with no id (bots, seed data) fall back to
//!   a note's name or aliases.
//! - Never automatic: people are only created, and accounts only linked, by
//!   the user (Create note, Link to…). Those writes go through the same path
//!   as any other note edit (`obsidian::save`: the outbox, or the file when
//!   there's no Obsidian source).
//! - Colour (16c): the note's `color`, else `fnv1a(key) % 256`, where the key
//!   is the person's email when one is known, otherwise the source account id.
//!   You are `people.my_color` (default 0, Gruvbox red). Colours are not
//!   spread apart; the UI owns the palette (`lib/people.ts`), this side only
//!   hands out indices.

use super::arg_str;
use crate::api::unknown;
use crate::api::Route;
use crate::clock::Clock;
use crate::domain::people::PersonNote;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::settings::PeopleSettings;
use crate::state::App;
use crate::sync::connectors::obsidian::save::{disk_text, save_note, Saved};
use crate::time::Ts;
#[cfg(test)]
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const PEOPLE_DIR: &str = "People";

/// Frontmatter field for each account kind.
const KINDS: &[&str] = &["slack", "email", "jira", "github"];

/// FNV-1a, 32 bit.
pub fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Hash colour for a key (an email or an account id).
pub fn hash_color(key: &str) -> u8 {
    (fnv1a(key) % 256) as u8
}

/// The account behind an author handle of a source, as `(kind, account)`.
/// Gmail relays stored as "address name" (`gmail::sender`) aren't anyone's
/// address, so they have no account.
pub fn account_of(source_kind: &str, handle: &str) -> Option<(String, String)> {
    let h = handle.trim();
    if h.is_empty() {
        return None;
    }
    let (kind, account) = match source_kind {
        "slack" => ("slack", h.to_string()),
        "jira" => ("jira", h.to_string()),
        "github" => ("github", h.to_lowercase()),
        "gmail" | "gcal" => match h.strip_prefix("github:") {
            Some(login) => ("github", login.to_lowercase()),
            None if h.contains(char::is_whitespace) || !h.contains('@') => return None,
            None => ("email", h.to_lowercase()),
        },
        _ => return None,
    };
    Some((kind.to_string(), account))
}

/// An account as a note lists it, normalised the way `account_of` stores it.
fn norm(kind: &str, v: &str) -> String {
    match kind {
        "email" | "github" => v.trim().to_lowercase(),
        _ => v.trim().to_string(),
    }
}

// ------------------------------------------------------------------ frontmatter

/// `(key, values)` in note order: a scalar is one value, a list each item.
pub type Fields = Vec<(String, Vec<String>)>;

/// Lines of the frontmatter block: (index of the opening `---`, index of the
/// closing one). None when the note has none.
fn fence(lines: &[&str]) -> Option<(usize, usize)> {
    let first = lines.first()?.trim_start_matches('\u{feff}').trim_end();
    if first != "---" {
        return None;
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| matches!(l.trim_end(), "---" | "..."))?
        .0;
    Some((0, end))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// `value  # comment` → `value` (not inside quotes).
fn strip_comment(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('"') || t.starts_with('\'') {
        return t;
    }
    match t.find(" #") {
        Some(i) => t[..i].trim_end(),
        None if t.starts_with('#') => "",
        None => t,
    }
}

/// `[a, "b, c", d]` → a | b, c | d.
fn inline_list(s: &str) -> Vec<String> {
    let inner = &s[1..s.len() - 1];
    let mut out = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match (quote, ch) {
            (None, '"' | '\'') => {
                quote = Some(ch);
                cur.push(ch);
            }
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(ch);
            }
            (None, ',') => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out.into_iter().map(|v| unquote(&v)).filter(|v| !v.is_empty()).collect()
}

/// The small YAML subset People notes use: `key: value`, `key: [a, b]` and
/// `key:` followed by `- item` lines. Anything else is skipped.
pub fn parse_frontmatter(text: &str) -> Fields {
    let lines: Vec<&str> = text.lines().collect();
    let Some((a, b)) = fence(&lines) else { return vec![] };
    let mut out: Fields = vec![];
    for line in &lines[a + 1..b] {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let t = line.trim();
        if let (true, Some(item)) = (
            indented || t.starts_with("- "),
            t.strip_prefix("- ").or_else(|| (t == "-").then_some("")),
        ) {
            if let Some(last) = out.last_mut() {
                let v = unquote(strip_comment(item));
                if !v.is_empty() {
                    last.1.push(v);
                }
            }
            continue;
        }
        if indented || t.starts_with('#') {
            continue;
        }
        let Some((k, rest)) = t.split_once(':') else { continue };
        let rest = strip_comment(rest);
        let values = if rest.is_empty() {
            vec![]
        } else if rest.starts_with('[') && rest.ends_with(']') {
            inline_list(rest)
        } else {
            vec![unquote(rest)]
        };
        out.push((k.trim().to_string(), values));
    }
    out
}

pub fn field<'a>(fields: &'a Fields, key: &str) -> &'a [String] {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_slice())
        .unwrap_or(&[])
}

fn yaml_value(v: &str) -> String {
    let special = v.starts_with(|c: char| "[]{},&*!|>'\"%@`#-?:".contains(c))
        || v.contains(": ")
        || v.contains(" #")
        || v.ends_with(':');
    if special {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// Add `value` to `key` in the note's frontmatter, keeping everything else as
/// written: a missing key is added before the closing `---`, a scalar becomes
/// a list, a list gets one more item. Returns None when it's already there.
pub fn add_to_frontmatter(text: &str, key: &str, value: &str) -> Option<String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let Some((a, b)) = fence(&refs) else {
        return Some(format!("---{nl}{key}: {}{nl}---{nl}{text}", yaml_value(value)));
    };
    if field(&parse_frontmatter(text), key)
        .iter()
        .any(|v| v.eq_ignore_ascii_case(value))
    {
        return None;
    }
    let at = (a + 1..b).find(|&i| {
        let l = &lines[i];
        !l.starts_with(' ') && !l.starts_with('\t') && l.split_once(':').is_some_and(|(k, _)| k.trim() == key)
    });
    match at {
        None => lines.insert(b, format!("{key}: {}", yaml_value(value))),
        Some(i) => {
            let rest = lines[i]
                .split_once(':')
                .map(|(_, r)| strip_comment(r).to_string())
                .unwrap_or_default();
            if rest.is_empty() {
                // Block list: after its last `- ` line, at the same indent.
                let mut last = i;
                let mut indent = "  ".to_string();
                for (j, l) in lines.iter().enumerate().take(b).skip(i + 1) {
                    let t = l.trim_start();
                    if t.starts_with('-') {
                        last = j;
                        indent = l[..l.len() - t.len()].to_string();
                    } else if !(l.starts_with(' ') || l.starts_with('\t')) {
                        break;
                    }
                }
                lines.insert(last + 1, format!("{indent}- {}", yaml_value(value)));
            } else if rest.starts_with('[') && rest.ends_with(']') {
                let inner = rest[1..rest.len() - 1].trim();
                let list = if inner.is_empty() {
                    yaml_value(value)
                } else {
                    format!("{inner}, {}", yaml_value(value))
                };
                lines[i] = format!("{key}: [{list}]");
            } else {
                lines[i] = format!("{key}: [{rest}, {}]", yaml_value(value));
            }
        }
    }
    let mut out = lines.join(nl);
    if text.ends_with('\n') {
        out.push_str(nl);
    }
    Some(out)
}

/// A new person's note: aliases plus the account it was created from (and,
/// for Slack, the profile's email).
pub fn stub_note(name: &str, alias: Option<&str>, account: Option<(&str, &str)>, email: Option<&str>) -> String {
    let mut s = String::from("---\n");
    match alias.map(str::trim).filter(|a| !a.is_empty() && *a != name) {
        Some(a) => s.push_str(&format!("aliases: [{}]\n", yaml_value(a))),
        None => s.push_str("aliases: []\n"),
    }
    if let Some((kind, acct)) = account {
        if kind == "email" {
            s.push_str(&format!("email: [{}]\n", yaml_value(acct)));
        } else {
            s.push_str(&format!("{kind}: {}\n", yaml_value(acct)));
        }
    }
    if let (Some(e), false) = (email, account.is_some_and(|(k, _)| k == "email")) {
        s.push_str(&format!("email: [{}]\n", yaml_value(e)));
    }
    s.push_str("---\n");
    s
}

/// A note name from a display name: no path or link characters, no trailing dots.
pub fn note_name(display: &str) -> String {
    let cleaned: String = display.chars().filter(|c| !"/\\:*?\"<>|#^[]".contains(*c)).collect();
    cleaned.trim().trim_end_matches('.').trim().to_string()
}

// ------------------------------------------------------------------ scan

/// Mirror `People/*.md` into `people` and `person_accounts` over a test's
/// connection. Returns how many notes were read.
#[cfg(test)]
pub fn scan(conn: &Connection, vault: &Path) -> Result<usize> {
    let notes = read_notes(vault);
    let tx = conn.unchecked_transaction()?;
    UnitOfWork::people(&crate::store::over(&tx)).mirror(&notes)?;
    tx.commit()?;
    Ok(notes.len())
}

/// `scan`, with the vault read before the database is touched.
pub fn rescan(app: &App) -> Result<usize> {
    rescan_vault(app, &app.vault)
}

/// `rescan` of a given vault (an Obsidian source's own root).
pub fn rescan_vault(app: &App, vault: &Path) -> Result<usize> {
    let notes = read_notes(vault);
    app.store().write(|uow| uow.people().mirror(&notes))?;
    Ok(notes.len())
}

/// Every note in `People/`, by path.
fn read_notes(vault: &Path) -> Vec<PersonNote> {
    let mut notes: Vec<PersonNote> = vec![];
    let Ok(rd) = std::fs::read_dir(vault.join(PEOPLE_DIR)) else {
        return notes;
    };
    for e in rd.flatten() {
        let p = e.path();
        let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
        let Some(stem) = p
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".md"))
        else {
            continue;
        };
        if !is_file || stem.starts_with('.') {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let f = parse_frontmatter(&text);
        let accounts = KINDS
            .iter()
            .flat_map(|kind| field(&f, kind).iter().map(|v| (kind.to_string(), norm(kind, v))))
            .filter(|(_, a)| !a.is_empty())
            .collect();
        notes.push(PersonNote {
            path: format!("{PEOPLE_DIR}/{stem}.md"),
            name: stem.to_string(),
            role: field(&f, "role").first().cloned(),
            color: field(&f, "color").first().and_then(|c| c.parse::<u8>().ok()),
            aliases: field(&f, "aliases").to_vec(),
            weight: field(&f, "weight")
                .first()
                .and_then(|w| w.parse::<f64>().ok())
                .filter(|w| w.is_finite())
                .map(|w| w.clamp(0.0, 1.0)),
            accounts,
        });
    }
    notes.sort_by(|a, b| a.path.cmp(&b.path));
    notes
}

// ------------------------------------------------------------------ resolving

#[derive(Debug, Clone)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub note_path: String,
    pub role: Option<String>,
    pub color: u8,
    /// The note's `weight` (0 to 1).
    pub weight: Option<f64>,
    /// Account kinds the note lists, in `KINDS` order.
    pub kinds: Vec<String>,
}

/// Who wrote a message or an item, as the UI needs it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Who {
    /// Palette index.
    pub color: u8,
    /// Matched to a People note.
    pub known: bool,
    pub me: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<i64>,
    /// The note's name, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The account behind the author, for Link to… (17b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Account {
    pub kind: String,
    pub id: String,
}

pub struct Resolver {
    by_account: HashMap<(String, String), i64>,
    /// Source profiles: (kind, account) → (real name, email).
    profiles: HashMap<(String, String), (Option<String>, Option<String>)>,
    by_name: HashMap<String, i64>,
    people: HashMap<i64, Person>,
    me: HashSet<(String, String)>,
    my_color: u8,
}

/// Your own accounts: settings `people.me`, each Slack source's own user id, and
/// each Jira source's email (the API token's owner).
fn my_accounts(r: &dyn Reads, settings: &PeopleSettings) -> Result<HashSet<(String, String)>> {
    let mut me: HashSet<(String, String)> = settings
        .me
        .iter()
        .filter_map(|s| s.split_once(':'))
        .filter(|(k, v)| KINDS.contains(k) && !v.trim().is_empty())
        .map(|(k, v)| (k.to_string(), norm(k, v)))
        .collect();
    for c in r.sources().of_kind("slack")?.into_iter().filter_map(|s| s.cursor) {
        if let Some(u) = serde_json::from_str::<Value>(&c)
            .ok()
            .and_then(|v| v.get("me").and_then(Value::as_str).map(str::to_owned))
        {
            if !u.is_empty() {
                me.insert(("slack".into(), u));
            }
        }
    }
    for s in r.sources().of_kind("jira")? {
        if let Some(e) = serde_json::from_str::<Value>(&s.config_json)
            .ok()
            .and_then(|v| v.get("email").and_then(Value::as_str).map(|e| norm("email", e)))
        {
            if !e.is_empty() {
                me.insert(("email".into(), e));
            }
        }
    }
    Ok(me)
}

impl Resolver {
    pub fn load(r: &dyn Reads, settings: &PeopleSettings) -> Result<Resolver> {
        let dir = r.people().directory()?;
        let mut people: HashMap<i64, Person> = HashMap::new();
        let mut by_name: HashMap<String, i64> = HashMap::new();
        let mut colors: HashMap<i64, Option<u8>> = HashMap::new();
        for row in dir.people {
            for n in std::iter::once(&row.name).chain(&row.aliases) {
                by_name.entry(n.to_lowercase()).or_insert(row.id);
            }
            colors.insert(row.id, row.color);
            people.insert(
                row.id,
                Person {
                    id: row.id,
                    name: row.name,
                    note_path: row.note_path,
                    role: row.role,
                    color: 0,
                    weight: row.weight,
                    kinds: vec![],
                },
            );
        }
        let mut by_account = HashMap::new();
        let mut keys: HashMap<i64, (Option<String>, Option<String>)> = HashMap::new();
        for a in dir.accounts {
            let Some(p) = people.get_mut(&a.person_id) else {
                continue;
            };
            if !p.kinds.contains(&a.kind) {
                p.kinds.push(a.kind.clone());
            }
            let k = keys.entry(a.person_id).or_default();
            if a.kind == "email" && k.0.is_none() {
                k.0 = Some(a.account.clone());
            } else if k.1.is_none() {
                k.1 = Some(a.account.clone());
            }
            by_account.insert((a.kind, a.account), a.person_id);
        }
        for p in people.values_mut() {
            p.kinds.sort_by_key(|k| KINDS.iter().position(|x| x == k));
            let key = keys
                .get(&p.id)
                .and_then(|(e, a)| e.clone().or_else(|| a.clone()))
                .unwrap_or_else(|| p.name.to_lowercase());
            p.color = colors.get(&p.id).copied().flatten().unwrap_or_else(|| hash_color(&key));
        }
        let profiles = dir
            .profiles
            .into_iter()
            .map(|p| ((p.kind, p.account), (p.real_name, p.email)))
            .collect();
        Ok(Resolver {
            by_account,
            profiles,
            by_name,
            people,
            me: my_accounts(r, settings)?,
            my_color: settings.my_color,
        })
    }

    /// The email a source's profile gives for an account (Slack), if any.
    pub fn profile_email(&self, kind: &str, account: &str) -> Option<&str> {
        self.profiles
            .get(&(kind.to_string(), account.to_string()))
            .and_then(|p| p.1.as_deref())
            .filter(|e| !e.is_empty())
    }

    /// The real name a source's profile gives for an account (Slack), if any.
    pub fn profile_name(&self, kind: &str, account: &str) -> Option<&str> {
        self.profiles
            .get(&(kind.to_string(), account.to_string()))
            .and_then(|p| p.0.as_deref())
            .filter(|n| !n.is_empty())
    }

    /// The note person an account belongs to: listed directly, or through its profile email.
    fn person_of(&self, kind: &str, account: &str) -> Option<i64> {
        self.by_account
            .get(&(kind.to_string(), account.to_string()))
            .or_else(|| {
                self.profile_email(kind, account)
                    .and_then(|e| self.by_account.get(&("email".to_string(), e.to_string())))
            })
            .copied()
    }

    pub fn person(&self, id: i64) -> Option<&Person> {
        self.people.get(&id)
    }

    pub fn people(&self) -> impl Iterator<Item = &Person> {
        self.people.values()
    }

    pub fn is_me(&self, kind: &str, account: &str) -> bool {
        self.me.contains(&(kind.to_string(), account.to_string()))
            || self
                .profile_email(kind, account)
                .is_some_and(|e| self.me.contains(&("email".to_string(), e.to_string())))
    }

    pub fn is_known(&self, kind: &str, account: &str) -> bool {
        self.person_of(kind, account).is_some()
    }

    /// The author of a message (`author_id` as the source stores it) or of an item (its handle).
    pub fn who(&self, source_kind: &str, author_id: Option<&str>, author: &str) -> Who {
        let account = author_id.and_then(|h| account_of(source_kind, h));
        if let Some((k, a)) = &account {
            if self.is_me(k, a) {
                return Who {
                    color: self.my_color,
                    known: true,
                    me: true,
                    person_id: None,
                    name: None,
                    account: None,
                };
            }
        }
        let pid = match &account {
            Some((k, a)) => self.person_of(k, a),
            // No id at all (bots, sample data): a note's name or alias.
            None if author_id.is_none() => self.by_name.get(&author.trim().to_lowercase()).copied(),
            None => None,
        };
        if let Some(p) = pid.and_then(|id| self.people.get(&id)) {
            return Who {
                color: p.color,
                known: true,
                me: false,
                person_id: Some(p.id),
                name: Some(p.name.clone()),
                account: None,
            };
        }
        // The email when the source gives one, else the account id (16c).
        let key = match &account {
            Some((k, a)) => self.profile_email(k, a).map(str::to_owned).unwrap_or_else(|| a.clone()),
            None => author.trim().to_lowercase(),
        };
        Who {
            color: hash_color(&key),
            known: false,
            me: false,
            person_id: None,
            name: None,
            account: account.map(|(kind, id)| Account { kind, id }),
        }
    }
}

pub fn resolver(app: &App) -> Result<Resolver> {
    let settings = app.settings();
    app.store().read(|r| Resolver::load(r, &settings.people))
}

// ------------------------------------------------------------------ new-person suggestions (17a)

#[derive(Serialize, Debug, Clone)]
pub struct Suggestion {
    pub kind: String,
    pub account: String,
    /// Latest display name seen for the account.
    pub name: String,
    /// The source profile's real name (Slack), for the strip and the new note's name.
    pub real_name: Option<String>,
    /// The account's email: itself for mail, the profile's for Slack.
    pub email: Option<String>,
    /// Source kind it was seen in, for the label ("Slack").
    pub source: String,
    /// Signals (split families count once) it was in within the window.
    pub signals: i64,
    pub color: u8,
}

/// Mail from a list or a system, not a person: never worth a People note.
fn bulk((kind, account): &(String, String)) -> bool {
    const LOCAL: &[&str] = &[
        "news",
        "newsletter",
        "info",
        "updates",
        "update",
        "hello",
        "team",
        "support",
        "help",
        "billing",
        "invoice",
        "receipts",
        "notifications",
        "notify",
        "alerts",
        "alert",
        "digest",
        "marketing",
        "mailer",
        "mailer-daemon",
        "postmaster",
        "bounce",
        "calendar",
        "drive-shares-dm-noreply",
        "do-not-reply",
        "donotreply",
        "admin",
        "security",
        "jobs",
        "careers",
        "events",
    ];
    kind == "email"
        && account.split('@').next().is_some_and(|l| {
            LOCAL.contains(&l) || l.contains("noreply") || l.contains("no-reply") || l.contains("bounce")
        })
}

/// Unknown senders seen in at least `suggest_after` signals within `suggest_days`,
/// not you and not snoozed, keyed by account.
pub fn suggestions(
    reads: &dyn Reads,
    r: &Resolver,
    settings: &PeopleSettings,
    now: Ts,
) -> Result<HashMap<(String, String), Suggestion>> {
    let since = now - jiff::SignedDuration::from_hours(24 * settings.suggest_days);
    let snoozed: HashSet<(String, String)> = reads.people().snoozed(now)?.into_iter().collect();
    let bots: HashSet<(String, String)> = reads.people().bots()?.into_iter().collect();
    /// Per account: signal roots, latest name, its time, its source.
    type Seen = (HashSet<i64>, String, Option<Ts>, String);
    let mut seen: HashMap<(String, String), Seen> = HashMap::new();
    for a in reads.people().authors_since(since)? {
        let Some(acct) = account_of(&a.source_kind, &a.handle) else {
            continue;
        };
        if r.is_me(&acct.0, &acct.1)
            || r.is_known(&acct.0, &acct.1)
            || snoozed.contains(&acct)
            || bots.contains(&acct)
            || bulk(&acct)
        {
            continue;
        }
        let e = seen
            .entry(acct)
            .or_insert_with(|| (HashSet::new(), String::new(), None, a.source_kind.clone()));
        e.0.insert(a.root);
        if e.2.is_none_or(|t| a.at >= t) {
            e.1 = a.name;
            e.2 = Some(a.at);
            e.3 = a.source_kind;
        }
    }
    Ok(seen
        .into_iter()
        .filter(|(_, v)| v.0.len() as i64 >= settings.suggest_after)
        .map(|((kind, account), (roots, name, _, source))| {
            let email = if kind == "email" {
                Some(account.clone())
            } else {
                r.profile_email(&kind, &account).map(str::to_owned)
            };
            let s = Suggestion {
                color: hash_color(email.as_deref().unwrap_or(&account)),
                real_name: r.profile_name(&kind, &account).map(str::to_owned),
                email,
                kind: kind.clone(),
                account: account.clone(),
                name,
                source,
                signals: roots.len() as i64,
            };
            ((kind, account), s)
        })
        .collect())
}

// ------------------------------------------------------------------ a topic's people

#[derive(Serialize, Debug)]
pub struct TopicPerson {
    /// The People note's person, when known.
    pub id: Option<i64>,
    pub name: String,
    pub initials: String,
    pub role: Option<String>,
    pub color: u8,
    pub known: bool,
    /// For Link to… on someone without a note.
    pub account: Option<Account>,
    /// Confirmed signals of this topic they wrote in.
    pub signals: i64,
    /// When they last did ("Thu 14:00", "Sep 25").
    pub last: Option<String>,
}

pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.as_slice() {
        [a, b, ..] => a
            .chars()
            .take(1)
            .chain(b.chars().take(1))
            .collect::<String>()
            .to_uppercase(),
        _ => name.chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// The topic's People panel: whoever wrote in its confirmed signals (by how
/// many, then how recently) and anyone linked to the topic by hand. You're
/// left out. Unknown senders show in their hash colour.
pub fn for_topic(
    reads: &dyn Reads,
    r: &Resolver,
    topic_id: i64,
    clock: &Clock,
    max: usize,
) -> Result<Vec<TopicPerson>> {
    let mut acc: Vec<(TopicPerson, HashSet<i64>, Option<Ts>)> = vec![];
    let mut index: HashMap<String, usize> = HashMap::new();
    for a in reads.people().topic_authors(topic_id)? {
        let (source, handle, author, item, at) = (a.source_kind, a.handle, a.name, a.item_id, a.at);
        let who = r.who(&source, handle.as_deref(), &author);
        if who.me || author.trim().is_empty() {
            continue;
        }
        let key = match (&who.person_id, &who.account) {
            (Some(id), _) => format!("p:{id}"),
            (None, Some(a)) => format!("a:{}:{}", a.kind, a.id),
            (None, None) => format!("n:{}", author.trim().to_lowercase()),
        };
        let i = *index.entry(key).or_insert_with(|| {
            let p = who.person_id.and_then(|id| r.person(id));
            let name = p.map(|p| p.name.clone()).unwrap_or_else(|| author.trim().to_string());
            acc.push((
                TopicPerson {
                    id: who.person_id,
                    initials: initials(&name),
                    name,
                    role: p.and_then(|p| p.role.clone()),
                    color: who.color,
                    known: who.known,
                    account: who.account.clone(),
                    signals: 0,
                    last: None,
                },
                HashSet::new(),
                None,
            ));
            acc.len() - 1
        });
        let e = &mut acc[i];
        e.1.insert(item);
        if e.2.is_none_or(|t| at > t) {
            e.2 = Some(at);
        }
    }
    // Linked by hand (topic_people), even with nothing confirmed yet.
    for crate::domain::people::LinkedPerson { id, name, role } in reads.people().linked_to_topic(topic_id)? {
        let known = r.person(id).is_some();
        let key = if known {
            format!("p:{id}")
        } else {
            format!("n:{}", name.trim().to_lowercase())
        };
        let i = match index.get(&key).copied().or_else(|| {
            if known {
                None
            } else {
                index.get(&format!("p:{id}")).copied()
            }
        }) {
            Some(i) => i,
            None => {
                let who = r.who("", None, &name);
                acc.push((
                    TopicPerson {
                        id: who.person_id.or(Some(id)),
                        initials: initials(&name),
                        name,
                        role: None,
                        color: who.color,
                        known: who.known,
                        account: None,
                        signals: 0,
                        last: None,
                    },
                    HashSet::new(),
                    None,
                ));
                index.insert(key, acc.len() - 1);
                acc.len() - 1
            }
        };
        if acc[i].0.role.is_none() {
            acc[i].0.role = role;
        }
    }
    acc.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.name.cmp(&b.0.name))
    });
    Ok(acc
        .into_iter()
        .take(max)
        .map(|(mut p, items, last)| {
            p.signals = items.len() as i64;
            p.last = last.map(|t| crate::queries::ts_label(t, clock, false));
            p
        })
        .collect())
}

// ------------------------------------------------------------------ agent context

/// What the assistant may know about a topic's people, one link away only:
/// each known person's note (gist), the notes that link to it, and which
/// topics their signals were filed under, how often and how recently.
/// The People notes `agent_context` quotes, to read before it runs.
pub fn agent_note_paths(reads: &dyn Reads, topic_id: i64, clock: &Clock) -> Result<Vec<String>> {
    let r = Resolver::load(reads, &PeopleSettings::default())?;
    Ok(for_topic(reads, &r, topic_id, clock, 9)?
        .into_iter()
        .filter(|p| p.known)
        .filter_map(|p| p.id.and_then(|id| r.person(id)).map(|p| p.note_path.clone()))
        .collect())
}

pub fn agent_context(
    reads: &dyn Reads,
    notes: &crate::ai::context::Notes,
    topic_id: i64,
    clock: &Clock,
) -> Result<String> {
    let r = Resolver::load(reads, &PeopleSettings::default())?;
    let mut out = String::new();
    for p in for_topic(reads, &r, topic_id, clock, 9)?
        .into_iter()
        .filter(|p| p.known)
    {
        let Some(person) = p.id.and_then(|id| r.person(id)) else {
            continue;
        };
        out.push_str(&format!("- {}", person.name));
        if let Some(role) = &person.role {
            out.push_str(&format!(" ({role})"));
        }
        out.push_str(&format!(", note {}\n", person.note_path));
        let gist = crate::ai::context::note_gist(notes.get(&person.note_path));
        if !gist.is_empty() {
            out.push_str(&format!("  Note: {gist}\n"));
        }
        let links = reads.people().notes_linking(&person.note_path, &person.name, 6)?;
        if !links.is_empty() {
            out.push_str(&format!("  Linked from: {}\n", links.join(", ")));
        }
        let filed: Vec<String> = reads
            .people()
            .filed_by(person.id, 5)?
            .into_iter()
            .filter(|f| f.1 > 0)
            .map(|(t, n, at)| format!("{t} ({n}, last {})", crate::queries::ts_label(at, clock, false)))
            .collect();
        if !filed.is_empty() {
            out.push_str(&format!("  Their signals filed under: {}\n", filed.join("; ")));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ actions

#[derive(Serialize, Debug)]
pub struct Hit {
    pub id: i64,
    pub name: String,
    pub note_path: String,
    pub role: Option<String>,
    pub color: u8,
    pub kinds: Vec<String>,
}

/// People notes matching `q` by name or alias (all of them when empty), best first.
pub fn search(reads: &dyn Reads, r: &Resolver, q: &str) -> Result<Vec<Hit>> {
    let needle = q.trim().to_lowercase();
    let aliases: HashMap<i64, Vec<String>> = reads
        .people()
        .directory()?
        .people
        .into_iter()
        .map(|p| (p.id, p.aliases))
        .collect();
    let mut hits: Vec<(u8, &Person)> = r
        .people()
        .filter_map(|p| {
            if needle.is_empty() {
                return Some((2, p));
            }
            let names = std::iter::once(p.name.to_lowercase())
                .chain(aliases.get(&p.id).into_iter().flatten().map(|a| a.to_lowercase()));
            names
                .map(|n| {
                    if n.starts_with(&needle) {
                        0
                    } else if n.split_whitespace().any(|w| w.starts_with(&needle)) {
                        1
                    } else if n.contains(&needle) {
                        2
                    } else {
                        9
                    }
                })
                .min()
                .filter(|&rank| rank < 9)
                .map(|rank| (rank, p))
        })
        .collect();
    hits.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    Ok(hits
        .into_iter()
        .take(8)
        .map(|(_, p)| Hit {
            id: p.id,
            name: p.name.clone(),
            note_path: p.note_path.clone(),
            role: p.role.clone(),
            color: p.color,
            kinds: p.kinds.clone(),
        })
        .collect())
}

fn account_args(args: &Value) -> Result<(String, String)> {
    let kind = arg_str(args, "kind")?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(msg(format!("unknown account kind '{kind}'")));
    }
    let account = norm(&kind, &arg_str(args, "account")?);
    if account.is_empty() {
        return Err(msg("empty account"));
    }
    Ok((kind, account))
}

/// After a write: the file is there (scan it) or the write is queued (record
/// what it will say, so colours follow now; the next scan has the final word).
fn after_write(app: &App, saved: Saved, rel: &str, name: &str, account: Option<&(String, String)>) -> Result<Value> {
    match saved {
        Saved::Conflict(_) => Err(msg(format!(
            "{rel} changed on disk while it was being edited; nothing was written. Try again."
        ))),
        Saved::Written => {
            rescan(app)?;
            Ok(json!({ "ok": true, "path": rel, "state": "written" }))
        }
        Saved::Queued(outbox_id) => {
            app.store().write(|uow| {
                uow.people()
                    .note_queued(rel, name, account.map(|(k, a)| (k.as_str(), a.as_str())))
            })?;
            Ok(json!({ "ok": true, "path": rel, "state": "queued", "outbox_id": outbox_id }))
        }
    }
}

/// 17b: add an account to an existing person's note.
pub fn link(app: &App, person_id: i64, kind: &str, account: &str) -> Result<Value> {
    let (rel, name) = app
        .store()
        .read(|r| r.people().note_of(person_id))?
        .ok_or_else(|| msg("that person has no note"))?;
    let before = disk_text(app, &rel).ok_or_else(|| msg(format!("{rel} isn't in the vault any more")))?;
    let pair = (kind.to_string(), account.to_string());
    match add_to_frontmatter(&before, kind, account) {
        None => {
            rescan(app)?;
            Ok(json!({ "ok": true, "path": rel, "state": "unchanged" }))
        }
        Some(after) => {
            let saved = save_note(app, &rel, Some(&before), &after)?;
            after_write(app, saved, &rel, &name, Some(&pair))
        }
    }
}

/// 17a Create note / 17b "Create person '…'": a stub note with the account.
pub fn create(app: &App, name: &str, alias: Option<&str>, account: Option<(String, String)>) -> Result<Value> {
    let name = note_name(name);
    if name.is_empty() {
        return Err(msg("a person needs a name"));
    }
    let rel = format!("{PEOPLE_DIR}/{name}.md");
    if app.vault.join(&rel).exists() {
        return Err(msg(format!("{rel} already exists: use Link to… instead")));
    }
    let email = match &account {
        Some((k, a)) if k != "email" => resolver(app)?.profile_email(k, a).map(str::to_owned),
        _ => None,
    };
    let text = stub_note(
        &name,
        alias,
        account.as_ref().map(|(k, a)| (k.as_str(), a.as_str())),
        email.as_deref(),
    );
    let saved = save_note(app, &rel, None, &text)?;
    after_write(app, saved, &rel, &name, account.as_ref())
}

/// Not now (17a): no suggestion for the account for a week.
pub fn snooze(uow: &dyn UnitOfWork, kind: &str, account: &str, now: Ts) -> Result<()> {
    uow.people()
        .snooze(kind, account, now + jiff::SignedDuration::from_hours(24 * 7))
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "search" => Route::Read(|r, ctx, args| {
            let people = Resolver::load(r, &ctx.settings.people)?;
            let q = args.get("q").and_then(Value::as_str).unwrap_or("");
            Ok(json!(search(r, &people, q)?))
        }),
        "snooze" => Route::Write(|uow, ctx, args| {
            let (kind, account) = account_args(args)?;
            snooze(uow, &kind, &account, ctx.clock.now())?;
            Ok(json!({ "ok": true }))
        }),
        _ => return None,
    })
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "link" => {
            let (kind, account) = account_args(args)?;
            link(app, super::arg_i64(args, "person_id")?, &kind, &account)
        }
        "create" => {
            let account = if args.get("kind").is_some_and(|k| !k.is_null()) {
                Some(account_args(args)?)
            } else {
                None
            };
            let alias = args.get("alias").and_then(Value::as_str);
            create(app, &arg_str(args, "name")?, alias, account)
        }
        "rescan" => Ok(json!({ "notes": rescan(app)? })),
        _ => Err(unknown(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn frontmatter_subset() {
        let text = "---\naliases: [Jordan, \"j, lee\"]\nslack: U04JRDN\nemail:\n  - Jordan.Lee@work.com\n  - jl@home.org # old\ncolor: 212          # optional\nteam: \"[[Platform]]\"\n---\nFree notes: not: fields\n";
        let f = parse_frontmatter(text);
        assert_eq!(field(&f, "aliases"), ["Jordan", "j, lee"]);
        assert_eq!(field(&f, "slack"), ["U04JRDN"]);
        assert_eq!(field(&f, "email"), ["Jordan.Lee@work.com", "jl@home.org"]);
        assert_eq!(field(&f, "color"), ["212"]);
        assert_eq!(field(&f, "team"), ["[[Platform]]"]);
        assert!(field(&f, "Free notes").is_empty());
        assert!(parse_frontmatter("no frontmatter\n---\nx: y\n---\n").is_empty());
    }

    #[test]
    fn adding_an_account_keeps_the_note_as_written() {
        // Missing key: added before the closing fence.
        let t = "---\naliases: [Sam]\n---\nBody\n";
        assert_eq!(
            add_to_frontmatter(t, "email", "sam.k@work.com").unwrap(),
            "---\naliases: [Sam]\nemail: sam.k@work.com\n---\nBody\n"
        );
        // Scalar becomes a list; inline list grows; block list grows at its indent.
        let t = "---\nslack: U1\n---\n";
        assert_eq!(
            add_to_frontmatter(t, "slack", "U2").unwrap(),
            "---\nslack: [U1, U2]\n---\n"
        );
        let t = "---\nemail: [a@x.com]\n---\n";
        assert_eq!(
            add_to_frontmatter(t, "email", "b@x.com").unwrap(),
            "---\nemail: [a@x.com, b@x.com]\n---\n"
        );
        let t = "---\nemail:\n    - a@x.com\nteam: X\n---\n";
        assert_eq!(
            add_to_frontmatter(t, "email", "b@x.com").unwrap(),
            "---\nemail:\n    - a@x.com\n    - b@x.com\nteam: X\n---\n"
        );
        // Already there (any case): nothing to write.
        assert!(add_to_frontmatter("---\nemail: [A@x.com]\n---\n", "email", "a@x.com").is_none());
        // No frontmatter yet.
        assert_eq!(
            add_to_frontmatter("Notes\n", "github", "jlee").unwrap(),
            "---\ngithub: jlee\n---\nNotes\n"
        );
        assert_eq!(
            add_to_frontmatter("", "github", "jlee").unwrap(),
            "---\ngithub: jlee\n---\n"
        );
        // CRLF stays CRLF.
        assert_eq!(
            add_to_frontmatter("---\r\nslack: U1\r\n---\r\n", "jira", "5f2c").unwrap(),
            "---\r\nslack: U1\r\njira: 5f2c\r\n---\r\n"
        );
    }

    #[test]
    fn stub_and_names() {
        assert_eq!(
            stub_note("Marco Rossi", Some("Marco R."), Some(("slack", "U04MARCO")), None),
            "---\naliases: [Marco R.]\nslack: U04MARCO\n---\n"
        );
        assert_eq!(
            stub_note(
                "Marco Rossi",
                Some("Marco R."),
                Some(("slack", "U04MARCO")),
                Some("marco@work.com")
            ),
            "---\naliases: [Marco R.]\nslack: U04MARCO\nemail: [marco@work.com]\n---\n"
        );
        assert_eq!(
            stub_note("Sam", None, Some(("email", "sam@x.com")), Some("x@y.z")),
            "---\naliases: []\nemail: [sam@x.com]\n---\n"
        );
        assert_eq!(note_name("Marco R."), "Marco R");
        assert_eq!(note_name(" a/b:c "), "abc");
        let f = parse_frontmatter(&stub_note("X", Some("@weird: name"), None, None));
        assert_eq!(field(&f, "aliases"), ["@weird: name"]);
    }

    #[test]
    fn accounts_from_handles() {
        assert_eq!(
            account_of("gmail", "Dana@Acme.com"),
            Some(("email".into(), "dana@acme.com".into()))
        );
        assert_eq!(
            account_of("gmail", "github:JLee"),
            Some(("github".into(), "jlee".into()))
        );
        assert_eq!(account_of("gmail", "noreply@x.com Jira"), None, "relays aren't people");
        assert_eq!(account_of("slack", "U1"), Some(("slack".into(), "U1".into())));
        assert_eq!(account_of("obsidian", "x"), None);
        assert!(bulk(&("email".into(), "newsletter@acme.com".into())));
        assert!(bulk(&("email".into(), "bounce-123@lists.x.com".into())));
        assert!(
            bulk(&("email".into(), "google-workspace-alerts-noreply@google.com".into())),
            "noreply as a suffix, no hyphen"
        );
        assert!(!bulk(&("email".into(), "dana@acme.com".into())));
        assert!(!bulk(&("slack".into(), "news".into())));
    }

    #[test]
    fn a_notes_weight_is_read_and_clamped_to_0_1() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(PEOPLE_DIR)).unwrap();
        for (name, w) in [("Kim", "0.7"), ("Lee", "3"), ("Sam", "high")] {
            std::fs::write(
                vault.path().join(PEOPLE_DIR).join(format!("{name}.md")),
                format!("---\nweight: {w}\n---\n"),
            )
            .unwrap();
        }
        let notes = read_notes(vault.path());
        let w = |n: &str| notes.iter().find(|p| p.name == n).unwrap().weight;
        assert_eq!((w("Kim"), w("Lee"), w("Sam")), (Some(0.7), Some(1.0), None));
    }

    #[test]
    fn hash_colours_are_stable() {
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("a"), 0xe40c_292c);
        assert_eq!(hash_color("jordan.lee@work.com"), hash_color("jordan.lee@work.com"));
    }

    fn db() -> Connection {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources (kind, tag, name, cursor) VALUES ('slack', 'work', 'Slack', '{\"me\":\"U0ME\"}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sources (kind, tag, name) VALUES ('gmail', 'work', 'Gmail')",
            [],
        )
        .unwrap();
        conn
    }

    fn write(vault: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(vault.join(PEOPLE_DIR)).unwrap();
        std::fs::write(vault.join(PEOPLE_DIR).join(format!("{name}.md")), text).unwrap();
    }

    #[test]
    fn scan_matches_authors_and_follows_note_edits() {
        let dir = tempfile::tempdir().unwrap();
        let v = dir.path();
        let conn = db();
        write(
            v,
            "Jordan Lee",
            "---\naliases: [Jordan]\nslack: U0JORDAN\nemail: [Jordan@Work.com]\n---\n",
        );
        write(v, "Kim", "---\ncolor: 212\nrole: Procurement\n---\n");
        assert_eq!(scan(&conn, v).unwrap(), 2);
        let s = PeopleSettings::default();
        let r = Resolver::load(&crate::store::over(&conn), &s).unwrap();

        let j = r.who("slack", Some("U0JORDAN"), "jordan");
        assert!(j.known && !j.me);
        assert_eq!(j.name.as_deref(), Some("Jordan Lee"));
        assert_eq!(
            j.color,
            hash_color("jordan@work.com"),
            "keyed by email when the note has one"
        );
        assert_eq!(
            r.who("gmail", Some("jordan@work.com"), "J. Lee"),
            j,
            "one person, one colour everywhere"
        );
        assert_eq!(
            r.who("gcal", None, "Kim").color,
            212,
            "the note's colour; no id falls back to the name"
        );
        let me = r.who("slack", Some("U0ME"), "zach");
        assert!(me.me);
        assert_eq!(me.color, 0);
        let marco = r.who("slack", Some("U0MARCO"), "Marco R.");
        assert!(!marco.known);
        assert_eq!(marco.color, hash_color("U0MARCO"));
        assert_eq!(
            marco.account,
            Some(Account {
                kind: "slack".into(),
                id: "U0MARCO".into()
            })
        );

        // The note changes in Obsidian: accounts follow; the row (and its id) stays.
        let id = j.person_id.unwrap();
        write(v, "Jordan Lee", "---\nslack: U0JORDAN2\n---\n");
        std::fs::remove_file(v.join("People/Kim.md")).unwrap();
        scan(&conn, v).unwrap();
        let r = Resolver::load(&crate::store::over(&conn), &s).unwrap();
        assert_eq!(r.who("slack", Some("U0JORDAN2"), "x").person_id, Some(id));
        assert!(!r.who("slack", Some("U0JORDAN"), "x").known);
        assert!(!r.who("gcal", None, "Kim").known);
        let kim: Option<String> = conn
            .query_row("SELECT note_path FROM people WHERE display_name = 'Kim'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kim, None, "a deleted note keeps its row but no path");
    }

    #[test]
    fn a_slack_profile_email_matches_a_note_and_colours_by_email() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db();
        write(dir.path(), "Marco Rossi", "---\nemail: [marco@work.com]\n---\n");
        scan(&conn, dir.path()).unwrap();
        let s = PeopleSettings {
            me: vec!["email:zach@work.com".into()],
            ..Default::default()
        };
        let unknown = Resolver::load(&crate::store::over(&conn), &s)
            .unwrap()
            .who("slack", Some("U0MARCO"), "Marco R.");
        assert!(!unknown.known);
        assert_eq!(
            unknown.color,
            hash_color("U0MARCO"),
            "no profile yet: keyed by the user id"
        );
        for (id, email) in [
            ("U0MARCO", "marco@work.com"),
            ("U0NEW", "new.person@work.com"),
            ("U0ZACH", "zach@work.com"),
        ] {
            conn.execute(
                "INSERT INTO account_profiles (kind, account, real_name, email) VALUES ('slack', ?1, 'X', ?2)",
                params![id, email],
            )
            .unwrap();
        }
        let r = Resolver::load(&crate::store::over(&conn), &s).unwrap();
        let marco = r.who("slack", Some("U0MARCO"), "Marco R.");
        assert!(marco.known, "matched through the profile email");
        assert_eq!(marco.color, hash_color("marco@work.com"));
        assert_eq!(
            marco,
            r.who("gmail", Some("marco@work.com"), "Marco"),
            "one person, one colour"
        );
        let new = r.who("slack", Some("U0NEW"), "New");
        assert!(!new.known);
        assert_eq!(
            new.color,
            hash_color("new.person@work.com"),
            "unknown, but keyed by the email the source gave"
        );
        assert!(r.who("slack", Some("U0ZACH"), "zach").me, "you, by your email");
    }

    #[test]
    fn your_jira_account_is_you() {
        let conn = db();
        conn.execute("INSERT INTO sources (kind, tag, name, config_json) VALUES ('jira', 'work', 'Jira', '{\"email\": \"Zach@Work.com\"}')", []).unwrap();
        conn.execute("INSERT INTO account_profiles (kind, account, real_name, email) VALUES ('jira', 'acc-me', 'Zach', 'zach@work.com')", []).unwrap();
        let r = Resolver::load(&crate::store::over(&conn), &PeopleSettings::default()).unwrap();
        assert!(r.who("jira", Some("acc-me"), "Zach").me);
        assert!(
            r.who("gmail", Some("zach@work.com"), "Zach").me,
            "the Jira email is yours everywhere"
        );
        assert!(!r.who("jira", Some("acc-other"), "Kim").me);
    }

    #[test]
    fn suggests_unknown_senders_seen_often_enough() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db();
        write(dir.path(), "Jordan", "---\nslack: U0JORDAN\n---\n");
        scan(&conn, dir.path()).unwrap();
        let now = crate::time::t("2026-09-30T12:00:00");
        for (n, (who, id, day)) in [
            ("Marco R.", "U0MARCO", 29),
            ("Marco R.", "U0MARCO", 28),
            ("Marco R.", "U0MARCO", 20),
            ("Jordan", "U0JORDAN", 29),
            ("Zach", "U0ME", 29),
        ]
        .iter()
        .enumerate()
        {
            conn.execute(
                "INSERT INTO items (source_id, external_id, kind, title, occurred_at) VALUES (1, ?1, 'thread', 't', ?2)",
                params![format!("x{n}"), format!("2026-09-{day}T09:00:00.000Z")],
            )
            .unwrap();
            let item = conn.last_insert_rowid();
            for _ in 0..3 {
                conn.execute(
                    "INSERT INTO item_messages (item_id, at, author, text, author_id) VALUES (?1, ?2, ?3, 'hi', ?4)",
                    params![item, format!("2026-09-{day}T09:00:00.000Z"), who, id],
                )
                .unwrap();
            }
        }
        let mut s = PeopleSettings {
            suggest_after: 2,
            ..Default::default()
        };
        let r = Resolver::load(&crate::store::over(&conn), &s).unwrap();
        let got = suggestions(&crate::store::over(&conn), &r, &s, now).unwrap();
        assert_eq!(got.len(), 1, "known people and you are never suggested");
        let m = &got[&("slack".to_string(), "U0MARCO".to_string())];
        assert_eq!(
            (m.signals, m.name.as_str()),
            (2, "Marco R."),
            "the one from 10 days ago is out of the window"
        );
        assert_eq!((m.real_name.as_deref(), m.email.as_deref()), (None, None));
        conn.execute("INSERT INTO account_profiles (kind, account, real_name, email) VALUES ('slack', 'U0MARCO', 'Marco Rossi', 'marco@work.com')", []).unwrap();
        let r2 = Resolver::load(&crate::store::over(&conn), &s).unwrap();
        let m = &suggestions(&crate::store::over(&conn), &r2, &s, now).unwrap()
            [&("slack".to_string(), "U0MARCO".to_string())];
        assert_eq!(
            (m.real_name.as_deref(), m.email.as_deref(), m.color),
            (
                Some("Marco Rossi"),
                Some("marco@work.com"),
                hash_color("marco@work.com")
            )
        );
        conn.execute("UPDATE account_profiles SET is_bot = 1 WHERE account = 'U0MARCO'", [])
            .unwrap();
        assert!(
            suggestions(&crate::store::over(&conn), &r2, &s, now)
                .unwrap()
                .is_empty(),
            "bots are never suggested"
        );
        s.suggest_after = 3;
        assert!(suggestions(&crate::store::over(&conn), &r, &s, now).unwrap().is_empty());
        s.suggest_after = 2;
        snooze(&crate::store::over(&conn), "slack", "U0MARCO", now).unwrap();
        assert!(
            suggestions(&crate::store::over(&conn), &r, &s, now).unwrap().is_empty(),
            "Not now hides it for 7 days"
        );
        assert_eq!(
            suggestions(
                &crate::store::over(&conn),
                &r,
                &s,
                now + jiff::SignedDuration::from_hours(24 * 8)
            )
            .unwrap()
            .len(),
            0,
            "and the old signals age out meanwhile"
        );
    }

    #[test]
    fn search_ranks_prefixes_and_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db();
        write(dir.path(), "Sam Kowalski", "---\nslack: U1\njira: 5f\n---\n");
        write(dir.path(), "Samir Patel", "---\nslack: U2\n---\n");
        write(dir.path(), "Dana Ruiz", "---\naliases: [Sammy]\n---\n");
        write(dir.path(), "Priya", "---\n---\n");
        scan(&conn, dir.path()).unwrap();
        let r = Resolver::load(&crate::store::over(&conn), &PeopleSettings::default()).unwrap();
        let names: Vec<String> = search(&crate::store::over(&conn), &r, "sam")
            .unwrap()
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert_eq!(names, ["Dana Ruiz", "Sam Kowalski", "Samir Patel"]);
        let hit = search(&crate::store::over(&conn), &r, "kowal").unwrap().remove(0);
        assert_eq!(
            (hit.note_path.as_str(), hit.kinds.clone()),
            ("People/Sam Kowalski.md", vec!["slack".to_string(), "jira".to_string()])
        );
        assert_eq!(search(&crate::store::over(&conn), &r, "").unwrap().len(), 4);
    }

    #[test]
    fn create_and_link_write_the_note_and_match_right_away() {
        let (app, _dir) = crate::ai::context::tests::app();
        app.db().execute("INSERT OR REPLACE INTO account_profiles (kind, account, real_name, email) VALUES ('slack', 'U0MARCO', 'Marco Rossi', 'marco.rossi@work.com')", []).unwrap();
        let v = create(
            &app,
            "Marco R.",
            Some("Marco R."),
            Some(("slack".into(), "U0MARCO".into())),
        )
        .unwrap();
        assert_eq!(v["path"], "People/Marco R.md");
        let text = std::fs::read_to_string(app.vault.join("People/Marco R.md")).unwrap();
        assert_eq!(
            text,
            "---\naliases: [Marco R.]\nslack: U0MARCO\nemail: [marco.rossi@work.com]\n---\n"
        );
        assert!(create(&app, "Marco R", None, None).is_err(), "never overwrites a note");
        let r = resolver(&app).unwrap();
        let who = r.who("slack", Some("U0MARCO"), "Marco R.");
        assert!(who.known);
        link(&app, who.person_id.unwrap(), "email", "marco@work.com").unwrap();
        let r = resolver(&app).unwrap();
        assert_eq!(r.who("gcal", Some("Marco@work.com"), "marco").person_id, who.person_id);
        assert_eq!(
            r.who("gcal", Some("Marco@work.com"), "marco").color,
            hash_color("marco.rossi@work.com"),
            "keyed by the note's first email"
        );
        let unchanged = link(&app, who.person_id.unwrap(), "email", "marco@work.com").unwrap();
        assert_eq!(unchanged["state"], "unchanged");
    }
}

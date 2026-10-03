//! Sample data from the design mocks, so every view renders before real sources
//! are connected. Times are placed relative to "today" so the day strip is live.
//!
//! Boundary rules apply to the sample data too: personal content only appears as
//! untitled Life blocks.
//!
//! Core records (sources, people, topics, today's calendar) live here. Each view
//! module adds its own items through its `seed` function.

use crate::state::App;
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{params, Connection};
use std::collections::HashMap;

pub struct Ctx<'a> {
    pub conn: &'a Connection,
    pub today: NaiveDate,
    pub sources: HashMap<&'static str, i64>,
    pub people: HashMap<&'static str, i64>,
    pub topics: HashMap<&'static str, i64>,
    counter: std::cell::Cell<i64>,
}

impl<'a> Ctx<'a> {
    /// Local timestamp `days` from today at "HH:MM".
    pub fn at(&self, days: i64, hm: &str) -> String {
        let t = NaiveTime::parse_from_str(hm, "%H:%M").expect("bad seed time");
        fmt(self.today.and_time(t) + Duration::days(days))
    }

    pub fn source(&self, kind: &str) -> i64 {
        *self.sources.get(kind).unwrap_or_else(|| panic!("no seed source {kind}"))
    }

    pub fn person(&self, name: &str) -> i64 {
        *self.people.get(name).unwrap_or_else(|| panic!("no seed person {name}"))
    }

    pub fn topic(&self, name: &str) -> i64 {
        *self.topics.get(name).unwrap_or_else(|| panic!("no seed topic {name}"))
    }

    /// Insert an item and return its id. Use `ItemSeed { ..Default::default() }`.
    pub fn item(&self, it: ItemSeed) -> i64 {
        let n = self.counter.get() + 1;
        self.counter.set(n);
        self.conn
            .execute(
                "INSERT INTO items (source_id, external_id, kind, title, body, summary, url, author_person_id,
                   occurred_at, ended_at, due_at, status, msg_count, owner, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    self.source(it.source),
                    format!("seed-{n}"),
                    it.kind,
                    it.title,
                    it.body,
                    it.summary,
                    it.url,
                    it.author.map(|a| self.person(a)),
                    it.occurred_at,
                    it.ended_at,
                    it.due_at,
                    it.status,
                    it.msg_count,
                    it.owner,
                    it.seen as i64,
                ],
            )
            .expect("seed item");
        self.conn.last_insert_rowid()
    }

    pub fn link(&self, item_id: i64, topic: &str, state: &str) {
        self.conn
            .execute(
                "INSERT INTO topic_links (item_id, topic_id, state, confidence) VALUES (?1, ?2, ?3, 0.9)",
                params![item_id, self.topic(topic), state],
            )
            .expect("seed link");
    }

    /// A Slack message: its author id is `U0` and the name's first word in
    /// capitals (`slack_id`), which the sample People notes list.
    pub fn slack(&self, item_id: i64, at: &str, author: &str, text: &str) -> i64 {
        self.slack_keyed(item_id, at, author, text, None, None)
    }

    /// `slack` with the message's key (Slack ts) and, for a thread reply, its parent's key.
    pub fn slack_keyed(&self, item_id: i64, at: &str, author: &str, text: &str, key: Option<&str>, reply_to: Option<&str>) -> i64 {
        self.conn
            .execute(
                "INSERT INTO item_messages (item_id, at, author, text, author_id, ext_key, reply_to) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![item_id, at, author, text, slack_id(author), key, reply_to],
            )
            .expect("seed message");
        self.conn.last_insert_rowid()
    }

    pub fn message(&self, item_id: i64, at: &str, author: &str, text: &str) {
        self.conn
            .execute(
                "INSERT INTO item_messages (item_id, at, author, text) VALUES (?1, ?2, ?3, ?4)",
                params![item_id, at, author, text],
            )
            .expect("seed message");
    }

    /// Work calendar event with a backing item. Returns the item id.
    pub fn meeting(&self, days: i64, start: &str, end: &str, title: &str, category: &str, attendees: &str, tentative: bool) -> i64 {
        let item = self.item(ItemSeed {
            source: "gcal",
            kind: "event",
            title: title.into(),
            occurred_at: self.at(days, start),
            ended_at: Some(self.at(days, end)),
            summary: if attendees.is_empty() { None } else { Some(attendees.into()) },
            seen: true,
            ..Default::default()
        });
        self.conn
            .execute(
                "INSERT INTO events (item_id, source_id, start, end, title, category, tentative, attendees)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![item, self.source("gcal"), self.at(days, start), self.at(days, end), title, category, tentative as i64, attendees],
            )
            .expect("seed event");
        item
    }

    /// Untitled busy block from a Proton "Limited view" link.
    pub fn life_block(&self, days: i64, start: &str, end: &str) {
        self.conn
            .execute(
                "INSERT INTO events (source_id, start, end, category, is_life_busy) VALUES (?1, ?2, ?3, 'life', 1)",
                params![self.source("protoncal"), self.at(days, start), self.at(days, end)],
            )
            .expect("seed life block");
    }
}

pub struct ItemSeed {
    pub source: &'static str,
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub author: Option<&'static str>,
    pub occurred_at: String,
    pub ended_at: Option<String>,
    pub due_at: Option<String>,
    pub status: &'static str,
    pub msg_count: Option<i64>,
    pub owner: Option<String>,
    pub seen: bool,
}

impl Default for ItemSeed {
    fn default() -> Self {
        ItemSeed {
            source: "gmail",
            kind: "email",
            title: String::new(),
            body: String::new(),
            summary: None,
            url: None,
            author: None,
            occurred_at: String::new(),
            ended_at: None,
            due_at: None,
            status: "open",
            msg_count: None,
            owner: None,
            seen: true,
        }
    }
}

/// Sample Slack user id for a name: "Marco R." → "U0MARCO".
pub fn slack_id(name: &str) -> String {
    let first = name.split_whitespace().next().unwrap_or("");
    format!("U0{}", first.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_uppercase())
}

/// Your Slack user id in the sample data (the Slack source's `me`).
pub const ME: &str = "Zach";

pub fn fmt(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn seed_if_empty(app: &App) -> anyhow::Result<()> {
    // Sample People, Topics and daily notes are files: never in a real vault.
    if !app.settings().obsidian_vault.is_empty() {
        return Ok(());
    }
    let mut conn = app.db();
    let n: i64 = conn.query_row("SELECT count(*) FROM sources", [], |r| r.get(0))?;
    if n > 0 {
        return Ok(());
    }
    let tx = conn.transaction()?;
    seed(&tx, app.clock.now().date(), &app.vault)?;
    tx.commit()?;
    Ok(())
}

/// People notes (17a/17b) for the sample people, in the stand-in vault, and
/// the rows `queries::people::scan` would make from them. Marco has none, so
/// Incoming can suggest one for him.
fn seed_people_notes(conn: &Connection, vault: &std::path::Path, people: &HashMap<&'static str, i64>) -> anyhow::Result<()> {
    let notes: [(&str, Option<&str>, Option<&str>, &str); 8] = [
        ("Jordan", Some("jordan.lee@work.com"), Some("jlee-work"), "[[Platform]]"),
        ("Priya", Some("priya.n@work.com"), None, "[[Data eng]]"),
        ("Sam", Some("sam.k@work.com"), None, "[[Accounts]]"),
        ("Kim", Some("kim@work.com"), None, "[[Procurement]]"),
        ("Lee", Some("lee.h@work.com"), None, "[[People team]]"),
        ("Ana", Some("ana@work.com"), None, "[[Finance]]"),
        ("Ravi", Some("ravi@datadoghq.com"), None, ""),
        ("Dana", Some("dana@acme.com"), None, ""),
    ];
    let dir = vault.join(crate::queries::people::PEOPLE_DIR);
    std::fs::create_dir_all(&dir)?;
    for (name, email, github, team) in notes {
        let id = people[name];
        let role: Option<String> = conn.query_row("SELECT role FROM people WHERE id = ?1", [id], |r| r.get(0))?;
        let slack = slack_id(name);
        let mut text = format!("---\naliases: []\nslack: {slack}\n");
        if let Some(e) = email {
            text.push_str(&format!("email: [{e}]\n"));
        }
        if let Some(g) = github {
            text.push_str(&format!("github: {g}\n"));
        }
        if let Some(r) = &role {
            text.push_str(&format!("role: {r}\n"));
        }
        if !team.is_empty() {
            text.push_str(&format!("team: \"{team}\"\n"));
        }
        text.push_str("---\n");
        let path = dir.join(format!("{name}.md"));
        if !path.exists() {
            std::fs::write(&path, text)?;
        }
        let rel = format!("{}/{name}.md", crate::queries::people::PEOPLE_DIR);
        conn.execute("UPDATE people SET note_path = ?2 WHERE id = ?1", params![id, rel])?;
        for (kind, acct) in [("slack", Some(slack.as_str())), ("email", email), ("github", github)] {
            if let Some(a) = acct {
                conn.execute("INSERT OR IGNORE INTO person_accounts (kind, account, person_id) VALUES (?1, ?2, ?3)", params![kind, a, id])?;
            }
        }
    }
    Ok(())
}

pub fn seed(conn: &Connection, today: NaiveDate, vault: &std::path::Path) -> anyhow::Result<()> {
    let mut sources = HashMap::new();
    for (kind, tag, name) in [
        ("gcal", "work", "Google Calendar"),
        ("slack", "work", "Slack"),
        ("jira", "work", "Jira"),
        ("github", "work", "GitHub"),
        ("obsidian", "work", "Obsidian"),
        ("gmail", "work", "Gmail"),
        ("sheets", "work", "Sheets"),
        ("notion", "work", "Notion"),
        ("protoncal", "personal", "Proton Calendar"),
        ("marvin", "personal", "Marvin"),
    ] {
        // Named apart from what Settings › Add source calls a new source, so a
        // real source never takes over the sample rows (sources match on kind and name).
        conn.execute("INSERT INTO sources (kind, tag, name) VALUES (?1, ?2, ?3)", params![kind, tag, format!("{name} (sample)")])?;
        sources.insert(kind, conn.last_insert_rowid());
    }

    let mut people = HashMap::new();
    for (name, role) in [
        ("Kim", "Procurement"),
        ("Ravi", "Datadog AE"),
        ("Ana", "Finance"),
        ("Priya", "Data eng manager"),
        ("Jordan", "Staff engineer"),
        ("Marco", "Senior engineer"),
        ("Sam", "Account manager"),
        ("Lee", "HR business partner"),
        ("Dana", "Acme · VP Eng"),
    ] {
        conn.execute("INSERT INTO people (display_name, role) VALUES (?1, ?2)", params![name, role])?;
        people.insert(name, conn.last_insert_rowid());
    }
    seed_people_notes(conn, vault, &people)?;
    conn.execute("UPDATE sources SET cursor = ?1 WHERE kind = 'slack'", [serde_json::json!({ "me": slack_id(ME) }).to_string()])?;

    let mut topics = HashMap::new();
    for (name, category, path) in [
        ("Acme outage", "work", "Topics/Acme outage.md"),
        ("Alerting · P2", "work", "Topics/Alerting P2.md"),
        ("Q4 headcount", "work", "Topics/Q4 headcount.md"),
        ("v1 ingest deprecation", "work", "Topics/v1 ingest deprecation.md"),
        ("Career", "jobs", "Topics/Career.md"),
        ("Datadog renewal", "work", "Topics/Datadog renewal.md"),
        ("Sr. EM offer", "work", "Topics/Sr. EM offer.md"),
        ("Q4 planning", "work", "Topics/Q4 planning.md"),
        ("Webhook reliability", "work", "Topics/Webhook reliability.md"),
        ("On-call load", "work", "Topics/On-call load.md"),
        ("Conference CFP", "work", "Topics/Conference CFP.md"),
    ] {
        conn.execute(
            "INSERT INTO topics (name, category, obsidian_path) VALUES (?1, ?2, ?3)",
            params![name, category, path],
        )?;
        topics.insert(name, conn.last_insert_rowid());
    }

    let ctx = Ctx { conn, today, sources, people, topics, counter: std::cell::Cell::new(0) };

    // Today's calendar (the strip in every mock). Now is ~10:52 in the mocks.
    ctx.meeting(0, "08:00", "08:30", "Coffee", "self", "", false);
    ctx.life_block(0, "08:30", "08:45");
    ctx.meeting(0, "09:00", "10:00", "Eng leads", "work", "Priya, Jordan, Marco +4", false);
    ctx.meeting(0, "10:15", "10:30", "Standup", "work", "Platform team", false);
    ctx.meeting(0, "10:30", "10:50", "1:1 Marco", "work", "Marco", false);
    let q4 = ctx.meeting(0, "11:00", "11:45", "Q4 headcount", "work", "Priya, Jordan, Lee +2", false);
    ctx.link(q4, "Q4 headcount", "confirmed");
    ctx.life_block(0, "12:00", "13:00");
    ctx.meeting(0, "13:00", "13:30", "Lunch", "self", "", false);
    ctx.meeting(0, "13:30", "14:30", "Staff loop", "work", "Directors", false);
    let vp = ctx.meeting(0, "14:30", "15:15", "VP sync", "work", "VP Eng", false);
    ctx.link(vp, "Acme outage", "confirmed");
    ctx.meeting(0, "15:30", "16:15", "Priya", "work", "Priya", false);
    let acme = ctx.meeting(0, "16:15", "16:45", "Acme call", "work", "Sam, Dana", true);
    ctx.link(acme, "Acme outage", "confirmed");
    let dd = ctx.meeting(0, "16:45", "17:15", "DD prep", "work", "Kim", false);
    ctx.link(dd, "Datadog renewal", "confirmed");
    ctx.life_block(0, "17:30", "18:00");
    ctx.meeting(0, "18:00", "19:00", "Walk", "self", "", false);

    let p2 = ctx.meeting(3, "10:00", "10:30", "Alert review", "work", "Jordan, SRE", false);
    ctx.link(p2, "Alerting · P2", "confirmed");

    crate::queries::topics::seed(&ctx, vault)?;
    crate::queries::incoming::seed(&ctx)?;
    crate::queries::priorities::seed(&ctx)?;
    crate::queries::notes::seed(&ctx)?;
    crate::queries::reflect::seed(&ctx, vault)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::settings::Settings;
    use crate::state::App;

    #[test]
    fn sample_data_never_goes_into_a_real_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let s = Settings { demo_data: true, obsidian_vault: vault.path().display().to_string(), ..Default::default() };
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), s, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(vault.path()).unwrap().count(), 0, "nothing written to the vault");
        let n: i64 = app.db().query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_real_source_added_later_gets_its_own_row() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings { demo_data: true, ..Default::default() }, dir.path()).unwrap();
        let cfg = serde_json::from_value::<crate::settings::SourceConfig>(serde_json::json!({ "kind": "slack", "tag": "work", "name": "Slack" })).unwrap();
        crate::sync::register_sources(&app.db(), &[cfg]).unwrap();
        let conn = app.db();
        let real: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'slack' AND name = 'Slack'", [], |r| r.get(0)).unwrap();
        let items: i64 = conn.query_row("SELECT count(*) FROM items WHERE source_id = ?1", [real], |r| r.get(0)).unwrap();
        assert_eq!(items, 0, "none of the sample Slack items");
        let cursor: Option<String> = conn.query_row("SELECT cursor FROM sources WHERE id = ?1", [real], |r| r.get(0)).unwrap();
        assert_eq!(cursor, None, "nor the sample cursor");
    }
}

//! Sample data from the design mocks, so every view renders before real sources
//! are connected. Times are placed relative to "today" so the day strip is live.
//!
//! Boundary rules apply to the sample data too: personal content only appears as
//! untitled Life blocks.
//!
//! Core records (sources, people, topics, today's calendar) live here. Each view
//! module adds its own items through its `seed` function.

use crate::clock::Clock;
use crate::domain::ports::UnitOfWork;
use crate::domain::sample::{SampleData, SampleEvent, SampleItem, SampleMessage};
use crate::state::App;
use crate::time::{Date, Due, Ts};
use jiff::civil::Time;
use jiff::ToSpan;
use std::cell::RefCell;
use std::collections::HashMap;

pub struct Ctx<'a> {
    pub uow: &'a dyn UnitOfWork,
    pub clock: Clock,
    pub today: Date,
    pub sources: HashMap<&'static str, i64>,
    pub people: HashMap<&'static str, i64>,
    pub topics: HashMap<&'static str, i64>,
    counter: std::cell::Cell<i64>,
    /// Meetings' items by (days from today, title).
    meetings: RefCell<HashMap<(i64, String), i64>>,
}

impl<'a> Ctx<'a> {
    /// "HH:MM" here, `days` from today.
    pub fn at(&self, days: i64, hm: &str) -> Ts {
        let t = Time::strptime("%H:%M", hm).expect("bad seed time");
        self.clock.from_local((self.today + days.days()).to_datetime(t))
    }

    /// The calendar day `days` from today.
    pub fn day(&self, days: i64) -> Date {
        self.today + days.days()
    }

    pub fn source(&self, kind: &str) -> i64 {
        *self
            .sources
            .get(kind)
            .unwrap_or_else(|| panic!("no seed source {kind}"))
    }

    pub fn person(&self, name: &str) -> i64 {
        *self.people.get(name).unwrap_or_else(|| panic!("no seed person {name}"))
    }

    pub fn topic(&self, name: &str) -> i64 {
        *self.topics.get(name).unwrap_or_else(|| panic!("no seed topic {name}"))
    }

    pub fn sample(&self) -> &dyn SampleData {
        self.uow.sample()
    }

    /// The item of a meeting `ctx.meeting` added.
    pub fn meeting_id(&self, days: i64, title: &str) -> i64 {
        *self
            .meetings
            .borrow()
            .get(&(days, title.to_owned()))
            .unwrap_or_else(|| panic!("no seed meeting {title} on day {days}"))
    }

    /// Insert an item and return its id. Use `ItemSeed { ..Default::default() }`.
    pub fn item(&self, it: ItemSeed) -> i64 {
        let n = self.counter.get() + 1;
        self.counter.set(n);
        self.sample()
            .item(&SampleItem {
                source_id: self.source(it.source),
                external_id: format!("seed-{n}"),
                kind: it.kind.into(),
                title: it.title,
                body: it.body,
                summary: it.summary,
                url: it.url,
                author_id: it.author.map(|a| self.person(a)),
                occurred_at: it.occurred_at,
                ended_at: it.ended_at,
                due: it.due,
                status: it.status.into(),
                msg_count: it.msg_count,
                owner: it.owner,
                seen: it.seen,
            })
            .expect("seed item")
    }

    pub fn link(&self, item_id: i64, topic: &str, state: &str) {
        self.sample()
            .link(item_id, self.topic(topic), state)
            .expect("seed link");
    }

    /// A Slack message: its author id is `U0` and the name's first word in
    /// capitals (`slack_id`), which the sample People notes list.
    pub fn slack(&self, item_id: i64, at: Ts, author: &str, text: &str) -> i64 {
        self.slack_keyed(item_id, at, author, text, None, None)
    }

    /// `slack` with the message's key (Slack ts) and, for a thread reply, its parent's key.
    pub fn slack_keyed(
        &self,
        item_id: i64,
        at: Ts,
        author: &str,
        text: &str,
        key: Option<&str>,
        reply_to: Option<&str>,
    ) -> i64 {
        let author_id = slack_id(author);
        self.sample()
            .message(&SampleMessage {
                item_id,
                at,
                author,
                text,
                author_id: Some(&author_id),
                ext_key: key,
                reply_to,
            })
            .expect("seed message")
    }

    pub fn message(&self, item_id: i64, at: Ts, author: &str, text: &str) {
        self.sample()
            .message(&SampleMessage {
                item_id,
                at,
                author,
                text,
                ..Default::default()
            })
            .expect("seed message");
    }

    /// Work calendar event with a backing item. Returns the item id.
    pub fn meeting(
        &self,
        days: i64,
        start: &str,
        end: &str,
        title: &str,
        category: &str,
        attendees: &str,
        tentative: bool,
    ) -> i64 {
        let item = self.item(ItemSeed {
            source: "gcal",
            kind: "event",
            title: title.into(),
            occurred_at: self.at(days, start),
            ended_at: Some(self.at(days, end)),
            summary: if attendees.is_empty() {
                None
            } else {
                Some(attendees.into())
            },
            seen: true,
            ..Default::default()
        });
        self.sample()
            .event(&SampleEvent {
                item_id: Some(item),
                source_id: self.source("gcal"),
                start: self.at(days, start),
                end: self.at(days, end),
                title: Some(title),
                category,
                tentative,
                attendees: Some(attendees),
                life: false,
            })
            .expect("seed event");
        // The guests that have a sample People note, as Google lists them (People: next meeting).
        let guests: Vec<serde_json::Value> = attendees
            .split(", ")
            .filter_map(|a| SAMPLE_NOTES.iter().find(|n| n.0 == a).and_then(|n| n.1))
            .map(|email| serde_json::json!({ "email": email, "responseStatus": "accepted" }))
            .collect();
        if !guests.is_empty() {
            let raw = serde_json::json!({ "attendees": guests }).to_string();
            self.sample()
                .patch_item(
                    item,
                    &crate::domain::sample::ItemPatch {
                        raw_json: Some(&raw),
                        ..Default::default()
                    },
                )
                .expect("seed meeting guests");
        }
        self.meetings.borrow_mut().insert((days, title.to_owned()), item);
        item
    }

    /// Make a calendar item an invite you haven't answered (14a/14b).
    pub fn invite(&self, item_id: i64) -> anyhow::Result<()> {
        let raw = serde_json::json!({
            "change": "invite",
            "attendees": [
                { "email": "alex.morgan@acme.com", "self": true, "responseStatus": "needsAction" },
                { "email": "sam.ortiz@acme.com", "organizer": true, "responseStatus": "accepted" },
            ],
        })
        .to_string();
        self.sample().patch_item(
            item_id,
            &crate::domain::sample::ItemPatch {
                raw_json: Some(&raw),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Untitled busy block from a Proton "Limited view" link.
    pub fn life_block(&self, days: i64, start: &str, end: &str) {
        self.sample()
            .event(&SampleEvent {
                source_id: self.source("protoncal"),
                start: self.at(days, start),
                end: self.at(days, end),
                category: "life",
                life: true,
                ..Default::default()
            })
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
    pub occurred_at: Ts,
    pub ended_at: Option<Ts>,
    pub due: Option<Due>,
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
            occurred_at: Ts::default(),
            ended_at: None,
            due: None,
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
    format!(
        "U0{}",
        first
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_uppercase()
    )
}

/// Your Slack user id in the sample data (the Slack source's `me`).
pub const ME: &str = "Zach";

pub fn seed_if_empty(app: &App) -> anyhow::Result<()> {
    // Sample People, Topics and daily notes are files: never in a real vault.
    if !app.settings().obsidian_vault.is_empty() {
        return Ok(());
    }
    // One unit of work; the sample notes are written into the stand-in vault
    // inside it, once, on an empty database.
    app.store().write(|uow| {
        if uow.sample().has_sources()? {
            return Ok(());
        }
        Ok(seed(uow, &app.clock, &app.vault)?)
    })?;
    Ok(())
}

/// The sample people with a People note: name, email, GitHub login, team.
const SAMPLE_NOTES: [(&str, Option<&str>, Option<&str>, &str); 8] = [
    ("Jordan", Some("jordan.lee@work.com"), Some("jlee-work"), "[[Platform]]"),
    ("Priya", Some("priya.n@work.com"), None, "[[Data eng]]"),
    ("Sam", Some("sam.k@work.com"), None, "[[Accounts]]"),
    ("Kim", Some("kim@work.com"), None, "[[Procurement]]"),
    ("Lee", Some("lee.h@work.com"), None, "[[People team]]"),
    ("Ana", Some("ana@work.com"), None, "[[Finance]]"),
    ("Ravi", Some("ravi@datadoghq.com"), None, ""),
    ("Dana", Some("dana@acme.com"), None, ""),
];

/// People notes (17a/17b) for the sample people, in the stand-in vault, and
/// the rows `queries::people::scan` would make from them. Marco has none, so
/// Incoming can suggest one for him.
fn seed_people_notes(
    sample: &dyn SampleData,
    vault: &std::path::Path,
    people: &HashMap<&'static str, (i64, &str)>,
) -> anyhow::Result<()> {
    let dir = vault.join(crate::domain::people::PEOPLE_DIR);
    std::fs::create_dir_all(&dir)?;
    for (name, email, github, team) in SAMPLE_NOTES {
        let (id, role) = people[name];
        let slack = slack_id(name);
        let mut text = format!("---\naliases: []\nslack: {slack}\n");
        if let Some(e) = email {
            text.push_str(&format!("email: [{e}]\n"));
        }
        if let Some(g) = github {
            text.push_str(&format!("github: {g}\n"));
        }
        text.push_str(&format!("role: {role}\n"));
        if !team.is_empty() {
            text.push_str(&format!("team: \"{team}\"\n"));
        }
        text.push_str("---\n");
        let path = dir.join(format!("{name}.md"));
        if !path.exists() {
            std::fs::write(&path, text)?;
        }
        let rel = format!("{}/{name}.md", crate::domain::people::PEOPLE_DIR);
        let accounts: Vec<(&str, &str)> = [("slack", Some(slack.as_str())), ("email", email), ("github", github)]
            .into_iter()
            .filter_map(|(k, a)| a.map(|a| (k, a)))
            .collect();
        sample.person_note(id, &rel, &accounts)?;
    }
    Ok(())
}

pub fn seed(uow: &dyn UnitOfWork, clock: &Clock, vault: &std::path::Path) -> anyhow::Result<()> {
    let sample = uow.sample();
    let today = clock.today();
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
        sources.insert(kind, sample.source(kind, tag, &format!("{name} (sample)"))?);
    }

    // The built-in source local todos live on, as the app makes it.
    sources.insert(
        "notes",
        uow.sources()
            .ensure_local("notes", crate::queries::notes::NOTES_SOURCE)?,
    );

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
        people.insert(name, (sample.person(name, role)?, role));
    }
    seed_people_notes(sample, vault, &people)?;
    let people: HashMap<&'static str, i64> = people.into_iter().map(|(k, (id, _))| (k, id)).collect();
    sample.set_cursor(sources["slack"], &serde_json::json!({ "me": slack_id(ME) }).to_string())?;

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
        ("Board prep", "work", "Topics/Board prep.md"),
        ("Staff loop", "work", "Topics/Staff loop.md"),
        ("Team growth", "work", "Topics/Team growth.md"),
    ] {
        topics.insert(name, sample.topic(name, category, path)?);
    }

    let ctx = Ctx {
        uow,
        clock: clock.clone(),
        today,
        sources,
        people,
        topics,
        counter: std::cell::Cell::new(0),
        meetings: RefCell::default(),
    };

    // Today's calendar (the strip in every mock). Now is ~10:52 in the mocks.
    ctx.meeting(0, "08:00", "08:30", "Coffee", "self", "", false);
    ctx.life_block(0, "08:30", "08:45");
    ctx.meeting(
        0,
        "09:00",
        "10:00",
        "Eng leads",
        "work",
        "Priya, Jordan, Marco +4",
        false,
    );
    ctx.meeting(0, "10:15", "10:30", "Standup", "work", "Platform team", false);
    ctx.meeting(0, "10:30", "10:50", "1:1 Marco", "work", "Marco", false);
    let q4 = ctx.meeting(
        0,
        "11:00",
        "11:45",
        "Q4 headcount",
        "work",
        "Priya, Jordan, Lee +2",
        false,
    );
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
        let s = Settings {
            demo_data: true,
            obsidian_vault: vault.path().display().to_string(),
            ..Default::default()
        };
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), s, dir.path()).unwrap();
        assert_eq!(
            std::fs::read_dir(vault.path()).unwrap().count(),
            0,
            "nothing written to the vault"
        );
        let n: i64 = app
            .db()
            .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_real_source_added_later_gets_its_own_row() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(
            crate::db::open_in_memory().unwrap(),
            Settings {
                demo_data: true,
                ..Default::default()
            },
            dir.path(),
        )
        .unwrap();
        let cfg = serde_json::from_value::<crate::settings::SourceConfig>(
            serde_json::json!({ "kind": "slack", "tag": "work", "name": "Slack" }),
        )
        .unwrap();
        crate::sync::register_sources(&app.db(), &[cfg]).unwrap();
        let conn = app.db();
        let real: i64 = conn
            .query_row(
                "SELECT id FROM sources WHERE kind = 'slack' AND name = 'Slack'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let items: i64 = conn
            .query_row("SELECT count(*) FROM items WHERE source_id = ?1", [real], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 0, "none of the sample Slack items");
        let cursor: Option<String> = conn
            .query_row("SELECT cursor FROM sources WHERE id = ?1", [real], |r| r.get(0))
            .unwrap();
        assert_eq!(cursor, None, "nor the sample cursor");
    }
}

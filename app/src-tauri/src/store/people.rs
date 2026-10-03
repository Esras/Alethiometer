//! People notes, accounts and source profiles.

use super::Sql;
use crate::domain::people::{AccountRow, Directory, LinkedPerson, People, PeopleViews, PersonNote, PersonRow, Profile, SeenAuthor, TopicAuthor};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use crate::error::Result;
use crate::time::Ts;

impl PeopleViews for Sql<'_> {
    fn directory(&self) -> Result<Directory> {
        let c = self.conn();
        let people = c
            .prepare_cached("SELECT id, display_name, note_path, role, color, aliases_json FROM people WHERE note_path IS NOT NULL ORDER BY note_path")?
            .query_map([], |r| {
                Ok(PersonRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    note_path: r.get(2)?,
                    role: r.get(3)?,
                    color: r.get(4)?,
                    aliases: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let accounts = c
            .prepare_cached("SELECT kind, account, person_id FROM person_accounts ORDER BY rowid")?
            .query_map([], |r| Ok(AccountRow { kind: r.get(0)?, account: r.get(1)?, person_id: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
        let profiles = c
            .prepare_cached("SELECT kind, account, real_name, email FROM account_profiles")?
            .query_map([], |r| Ok(Profile { kind: r.get(0)?, account: r.get(1)?, real_name: r.get(2)?, email: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Directory { people, accounts, profiles })
    }

    fn snoozed(&self, now: Ts) -> Result<Vec<(String, String)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT kind, account FROM person_snoozes WHERE until > ?1")?
            .query_map([now], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn bots(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT kind, account FROM account_profiles WHERE is_bot = 1")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn authors_since(&self, since: Ts) -> Result<Vec<SeenAuthor>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT s.kind, m.author_id, m.author, coalesce(i.parent_id, i.id), m.at
                   FROM item_messages m JOIN items i ON i.id = m.item_id JOIN sources s ON s.id = i.source_id
                  WHERE m.author_id IS NOT NULL AND m.context = 0 AND m.at >= ?1 AND s.tag = 'work'
                 UNION ALL
                 SELECT s.kind, h.handle, p.display_name, coalesce(i.parent_id, i.id), i.occurred_at
                   FROM items i JOIN sources s ON s.id = i.source_id
                   JOIN person_handles h ON h.person_id = i.author_person_id AND h.source_kind = s.kind
                   JOIN people p ON p.id = i.author_person_id
                  WHERE i.occurred_at >= ?1 AND s.tag = 'work'",
            )?
            .query_map([since], |r| Ok(SeenAuthor { source_kind: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, root: r.get(3)?, at: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn topic_authors(&self, topic_id: i64) -> Result<Vec<TopicAuthor>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT s.kind, m.author_id, m.author, i.id, m.at
                   FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                   JOIN item_messages m ON m.item_id = i.id
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND m.context = 0 AND s.tag = 'work'
                 UNION ALL
                 SELECT s.kind, h.handle, p.display_name, i.id, i.occurred_at
                   FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                   JOIN people p ON p.id = i.author_person_id
                   LEFT JOIN person_handles h ON h.person_id = p.id AND h.source_kind = s.kind
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'",
            )?
            .query_map([topic_id], |r| Ok(TopicAuthor { source_kind: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, item_id: r.get(3)?, at: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn linked_to_topic(&self, topic_id: i64) -> Result<Vec<LinkedPerson>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT p.id, p.display_name, p.role FROM topic_people tp JOIN people p ON p.id = tp.person_id WHERE tp.topic_id = ?1")?
            .query_map([topic_id], |r| Ok(LinkedPerson { id: r.get(0)?, name: r.get(1)?, role: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn notes_linking(&self, note_path: &str, name: &str, limit: usize) -> Result<Vec<String>> {
        let stem = note_path.trim_end_matches(".md");
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT DISTINCT json_extract(i.raw_json, '$.path') FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE s.kind = 'obsidian' AND s.tag = 'work' AND i.kind = 'note' AND i.status = 'open'
                    AND json_extract(i.raw_json, '$.path') != ?3
                    AND (instr(i.body, '[[' || ?1 || ']]') OR instr(i.body, '[[' || ?1 || '|') OR instr(i.body, '[[' || ?2 || ']]') OR instr(i.body, '[[' || ?2 || '|'))
                  LIMIT ?4",
            )?
            .query_map(params![stem, name, note_path, limit as i64], |x| x.get::<_, Option<String>>(0))?
            .filter_map(|x| x.transpose())
            .collect::<rusqlite::Result<_>>()?)
    }

    fn filed_by(&self, person_id: i64, limit: usize) -> Result<Vec<(String, i64, Ts)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT t.name, count(DISTINCT i.id), max(m.at) FROM topic_links tl
                   JOIN topics t ON t.id = tl.topic_id JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                   JOIN item_messages m ON m.item_id = i.id
                  WHERE tl.state = 'confirmed' AND s.tag = 'work' AND m.context = 0 AND m.author_id IS NOT NULL
                    AND EXISTS (SELECT 1 FROM person_accounts pa WHERE pa.person_id = ?1 AND pa.account = CASE pa.kind
                          WHEN 'email' THEN lower(m.author_id) WHEN 'github' THEN lower(replace(m.author_id, 'github:', '')) ELSE m.author_id END)
                  GROUP BY t.id ORDER BY max(m.at) DESC LIMIT ?2",
            )?
            .query_map(params![person_id, limit as i64], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn note_of(&self, person_id: i64) -> Result<Option<(String, String)>> {
        Ok(self
            .conn()
            .query_row("SELECT note_path, display_name FROM people WHERE id = ?1 AND note_path IS NOT NULL", [person_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    }
}

impl People for Sql<'_> {
    fn mirror(&self, notes: &[PersonNote]) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM person_accounts", [])?;
        let mut keep: HashSet<i64> = HashSet::new();
        for n in notes {
            let aliases = serde_json::to_string(&n.aliases)?;
            let existing: Option<i64> = c.query_row("SELECT id FROM people WHERE note_path = ?1", [&n.path], |r| r.get(0)).optional()?;
            let id = match existing {
                Some(id) => {
                    c.execute(
                        "UPDATE people SET display_name = ?2, role = coalesce(?3, role), color = ?4, aliases_json = ?5 WHERE id = ?1",
                        params![id, n.name, n.role, n.color, aliases],
                    )?;
                    id
                }
                None => {
                    c.execute(
                        "INSERT INTO people (display_name, role, note_path, color, aliases_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![n.name, n.role, n.path, n.color, aliases],
                    )?;
                    c.last_insert_rowid()
                }
            };
            for (kind, account) in &n.accounts {
                c.execute("INSERT OR IGNORE INTO person_accounts (kind, account, person_id) VALUES (?1, ?2, ?3)", params![kind, account, id])?;
            }
            keep.insert(id);
        }
        let noted: Vec<i64> = c.prepare("SELECT id FROM people WHERE note_path IS NOT NULL")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for id in noted.into_iter().filter(|id| !keep.contains(id)) {
            c.execute("UPDATE people SET note_path = NULL, color = NULL WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    fn note_queued(&self, path: &str, name: &str, account: Option<(&str, &str)>) -> Result<()> {
        let c = self.conn();
        let id: i64 = match c.query_row("SELECT id FROM people WHERE note_path = ?1", [path], |r| r.get(0)).optional()? {
            Some(id) => id,
            None => {
                c.execute("INSERT INTO people (display_name, note_path) VALUES (?1, ?2)", params![name, path])?;
                c.last_insert_rowid()
            }
        };
        if let Some((kind, acct)) = account {
            c.execute("INSERT OR IGNORE INTO person_accounts (kind, account, person_id) VALUES (?1, ?2, ?3)", params![kind, acct, id])?;
        }
        Ok(())
    }

    fn snooze(&self, kind: &str, account: &str, until: Ts) -> Result<()> {
        self.conn().execute(
            "INSERT INTO person_snoozes (kind, account, until) VALUES (?1, ?2, ?3) ON CONFLICT (kind, account) DO UPDATE SET until = ?3",
            params![kind, account, until],
        )?;
        Ok(())
    }
}

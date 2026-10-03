//! People notes, accounts and source profiles.

use super::Sql;
use crate::domain::people::{AccountRow, Directory, LinkedPerson, PeopleViews, PersonRow, Profile, SeenAuthor, TopicAuthor};
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
}

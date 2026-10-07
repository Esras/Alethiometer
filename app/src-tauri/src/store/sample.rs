//! Sample data rows (`crate::seed`).

use super::Sql;
use crate::domain::sample::{ItemPatch, SampleData, SampleEvent, SampleItem, SampleMessage, TodoExtras};
use crate::error::Result;
use crate::time::Due;
use rusqlite::params;

impl Sql<'_> {
    fn inserted(&self, sql: &str, args: impl rusqlite::Params) -> Result<i64> {
        self.conn().execute(sql, args)?;
        Ok(self.conn().last_insert_rowid())
    }
}

impl SampleData for Sql<'_> {
    fn has_sources(&self) -> Result<bool> {
        Ok(self.conn().query_row("SELECT EXISTS (SELECT 1 FROM sources)", [], |r| r.get(0))?)
    }

    fn source(&self, kind: &str, tag: &str, name: &str) -> Result<i64> {
        self.inserted("INSERT INTO sources (kind, tag, name) VALUES (?1, ?2, ?3)", params![kind, tag, name])
    }

    fn set_cursor(&self, source_id: i64, cursor: &str) -> Result<()> {
        self.conn().execute("UPDATE sources SET cursor = ?2 WHERE id = ?1", params![source_id, cursor])?;
        Ok(())
    }

    fn person(&self, name: &str, role: &str) -> Result<i64> {
        self.inserted("INSERT INTO people (display_name, role) VALUES (?1, ?2)", params![name, role])
    }

    fn person_note(&self, person_id: i64, note_path: &str, accounts: &[(&str, &str)]) -> Result<()> {
        self.conn().execute("UPDATE people SET note_path = ?2 WHERE id = ?1", params![person_id, note_path])?;
        for (kind, account) in accounts {
            self.conn().execute("INSERT OR IGNORE INTO person_accounts (kind, account, person_id) VALUES (?1, ?2, ?3)", params![kind, account, person_id])?;
        }
        Ok(())
    }

    fn profile(&self, kind: &str, account: &str, real_name: &str, email: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO account_profiles (kind, account, real_name, email) VALUES (?1, ?2, ?3, ?4)",
            params![kind, account, real_name, email],
        )?;
        Ok(())
    }

    fn topic(&self, name: &str, category: &str, note_path: &str) -> Result<i64> {
        self.inserted("INSERT INTO topics (name, category, obsidian_path) VALUES (?1, ?2, ?3)", params![name, category, note_path])
    }

    fn topic_score(&self, topic_id: i64, score: f64, pills_json: &str) -> Result<()> {
        self.conn().execute("UPDATE topics SET score = ?2, pills_json = ?3 WHERE id = ?1", params![topic_id, score, pills_json])?;
        Ok(())
    }

    fn todo_extras(&self, item_id: i64, x: &TodoExtras) -> Result<()> {
        let c = self.conn();
        c.execute(
            "UPDATE items SET raw_json = ?2, sched_day = ?3, source_created_at = ?4 WHERE id = ?1",
            params![item_id, x.raw_json, x.sched_day, x.source_created_at],
        )?;
        if let Some((m, by)) = x.estimate {
            c.execute(
                "INSERT INTO todo_estimates (item_id, minutes, by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![item_id, m, by, x.created_at],
            )?;
        }
        c.execute("UPDATE topic_links SET created_at = ?2, updated_at = ?2 WHERE item_id = ?1", params![item_id, x.created_at])?;
        c.execute("UPDATE items SET created_at = ?2, updated_at = ?3 WHERE id = ?1", params![item_id, x.created_at, x.updated_at])?;
        Ok(())
    }

    fn topic_priority(&self, topic_id: i64, level: &str) -> Result<()> {
        self.conn().execute("UPDATE topics SET priority = ?2 WHERE id = ?1", params![topic_id, level])?;
        Ok(())
    }

    fn topic_person(&self, topic_id: i64, person_id: i64) -> Result<()> {
        self.conn().execute("INSERT INTO topic_people (topic_id, person_id) VALUES (?1, ?2)", params![topic_id, person_id])?;
        Ok(())
    }

    fn topic_repo(&self, topic_id: i64, repo: &str) -> Result<()> {
        self.conn().execute("INSERT INTO topic_repos (topic_id, repo) VALUES (?1, ?2)", params![topic_id, repo])?;
        Ok(())
    }

    fn item(&self, it: &SampleItem) -> Result<i64> {
        let (due_at, due_day) = Due::columns(it.due);
        self.inserted(
            "INSERT INTO items (source_id, external_id, kind, title, body, summary, url, author_person_id,
               occurred_at, ended_at, due_at, status, msg_count, owner, seen, due_day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                it.source_id,
                it.external_id,
                it.kind,
                it.title,
                it.body,
                it.summary,
                it.url,
                it.author_id,
                it.occurred_at,
                it.ended_at,
                due_at,
                it.status,
                it.msg_count,
                it.owner,
                it.seen as i64,
                due_day,
            ],
        )
    }

    fn patch_item(&self, id: i64, p: &ItemPatch) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET title = coalesce(?2, title), body = coalesce(?3, body), summary = coalesce(?4, summary),
                    url = coalesce(?5, url), raw_json = coalesce(?6, raw_json)
              WHERE id = ?1",
            params![id, p.title, p.body, p.summary, p.url, p.raw_json],
        )?;
        Ok(())
    }

    fn link(&self, item_id: i64, topic_id: i64, state: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence) VALUES (?1, ?2, ?3, 0.9)",
            params![item_id, topic_id, state],
        )?;
        Ok(())
    }

    fn message(&self, m: &SampleMessage) -> Result<i64> {
        self.inserted(
            "INSERT INTO item_messages (item_id, at, author, text, author_id, ext_key, reply_to) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                m.item_id,
                m.at,
                m.author,
                m.text,
                m.author_id,
                // Every message has a key (`message_keys`): a hash when the sample gives none.
                m.ext_key.map(str::to_owned).unwrap_or_else(|| crate::domain::splits::content_key(&m.at.to_db(), m.author, m.text)),
                m.reply_to
            ],
        )
    }

    fn event(&self, e: &SampleEvent) -> Result<()> {
        if e.life {
            self.conn().execute(
                "INSERT INTO events (source_id, start, end, category, is_life_busy) VALUES (?1, ?2, ?3, 'life', 1)",
                params![e.source_id, e.start, e.end],
            )?;
        } else {
            self.conn().execute(
                "INSERT INTO events (item_id, source_id, start, end, title, category, tentative, attendees)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![e.item_id, e.source_id, e.start, e.end, e.title, e.category, e.tentative as i64, e.attendees],
            )?;
        }
        Ok(())
    }

    fn note_line(&self, page_date: &str, position: i64, kind: &str, text: &str, target_json: &str, state: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO notes_lines (page_date, position, kind, text, target_json, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![page_date, position, kind, text, target_json, state],
        )?;
        Ok(())
    }

    fn carry(&self, item_id: i64, period: &str, action: Option<&str>) -> Result<()> {
        match action {
            Some(a) => self.conn().execute("INSERT INTO carry (item_id, period, action) VALUES (?1, ?2, ?3)", params![item_id, period, a])?,
            None => self.conn().execute("INSERT INTO carry (item_id, period) VALUES (?1, ?2)", params![item_id, period])?,
        };
        Ok(())
    }

    fn reflection(&self, scope: &str, period_start: &str, note_path: &str, summary: Option<&str>) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO reflections (scope, period_start, lens, obsidian_path, summary) VALUES (?1, ?2, 'work', ?3, ?4)",
            params![scope, period_start, note_path, summary],
        )?;
        Ok(())
    }
}

//! Agents tab state (`agent_*` tables, rows as JSON) and the agents source's signals.

use super::Sql;
use crate::agents::model::{Group, Row, Session};
use crate::domain::agents::{AgentSignal, AgentState, AgentViews, Archived, ArchivedMatch, HostState, PrRow, SavedAgents, TaskRow, TicketRow};
use crate::time::Ts;
use crate::error::Result;
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;

impl Sql<'_> {
    /// JSON documents from a one-column query; ones that don't parse are left out.
    fn docs<T: DeserializeOwned>(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<T>> {
        Ok(self
            .conn()
            .prepare_cached(sql)?
            .query_map(args, |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok().and_then(|d| serde_json::from_str(&d).ok()))
            .collect())
    }
}

const ARCHIVED: &str = "SELECT id, kind, source, project, group_name, group_id, archived_at, data FROM agent_archive";

fn archived_from(r: &rusqlite::Row) -> rusqlite::Result<Archived> {
    Ok(Archived { id: r.get(0)?, kind: r.get(1)?, source: r.get(2)?, project: r.get(3)?, group_name: r.get(4)?, group_id: r.get(5)?, at: r.get(6)?, data: r.get(7)? })
}

impl AgentViews for Sql<'_> {
    fn saved(&self) -> Result<SavedAgents> {
        let hosts = self
            .conn()
            .prepare_cached("SELECT id, seq, turn FROM agent_hosts")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?
            .filter_map(|r| r.ok())
            .map(|(id, seq, turn)| (id, HostState { seq: seq as u64, turn: serde_json::from_str(&turn).unwrap_or_default() }))
            .collect();
        let last_row_ids = self
            .conn()
            .prepare_cached("SELECT thread, max(id) FROM agent_rows GROUP BY thread")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(SavedAgents {
            groups: self.docs("SELECT data FROM agent_groups", [])?,
            sessions: self.docs("SELECT data FROM agent_sessions", [])?,
            hosts,
            last_row_ids,
            archived: self
                .conn()
                .prepare_cached("SELECT id FROM agent_archive")?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?,
            snoozed: self
                .conn()
                .prepare_cached("SELECT grp, until FROM agent_nudges")?
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Ts>(1)?)))?
                .collect::<rusqlite::Result<_>>()?,
        })
    }

    fn transcript(&self, thread: &str) -> Result<Vec<Row>> {
        self.docs("SELECT data FROM agent_rows WHERE thread = ?1 ORDER BY id", [thread])
    }

    fn ticket(&self, key: &str) -> Result<Option<TicketRow>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT i.summary, i.url, i.status = 'done' FROM items i JOIN sources s ON s.id = i.source_id WHERE s.kind = 'jira' AND i.external_id = ?1",
                [key],
                |r| Ok(TicketRow { summary: r.get(0)?, url: r.get(1)?, done: r.get(2)? }),
            )
            .optional()?)
    }

    fn archived_matching(&self, q: &str, limit: usize) -> Result<Vec<ArchivedMatch>> {
        const ROW: &str = "SELECT json_array(r.thread, r.data) FROM agent_rows r
             WHERE r.thread IN ('s:' || a.id, 'r:' || json_extract(a.data, '$.room.id'))
               AND instr(lower(coalesce(json_extract(r.data, '$.text'), '') || char(10) || coalesce(json_extract(r.data, '$.tool.detail'), '') || char(10) || coalesce(json_extract(r.data, '$.tool.result'), '')), ?1) > 0
             ORDER BY r.id LIMIT 1";
        let sql = format!(
            "SELECT a.id, a.kind, a.source, a.project, a.group_name, a.group_id, a.archived_at, a.data, ({ROW}) FROM agent_archive a
              WHERE instr(lower(coalesce(json_extract(a.data, '$.name'), '') || char(10) || coalesce(json_extract(a.data, '$.goal'), '') || char(10) || coalesce(json_extract(a.data, '$.title'), '')), ?1) > 0
                 OR EXISTS ({ROW})
              ORDER BY a.archived_at DESC LIMIT ?2"
        );
        Ok(self
            .conn()
            .prepare_cached(&sql)?
            .query_map(params![q.to_ascii_lowercase(), limit as i64], |r| {
                let row: Option<String> = r.get(8)?;
                Ok(ArchivedMatch { item: archived_from(r)?, row: row.and_then(|j| serde_json::from_str::<(String, String)>(&j).ok()) })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn archived(&self, id: &str) -> Result<Option<Archived>> {
        Ok(self.conn().query_row(&format!("{ARCHIVED} WHERE id = ?1"), [id], archived_from).optional()?)
    }

    fn archived_members(&self, group_id: &str) -> Result<Vec<Archived>> {
        Ok(self
            .conn()
            .prepare_cached(&format!("{ARCHIVED} WHERE kind = 'session' AND group_id = ?1 ORDER BY archived_at"))?
            .query_map([group_id], archived_from)?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn archived_sessions(&self) -> Result<usize> {
        Ok(self.conn().query_row("SELECT count(*) FROM agent_archive WHERE kind = 'session'", [], |r| r.get::<_, i64>(0))? as usize)
    }

    fn pr(&self, branch: Option<&str>, key: Option<&str>) -> Result<Option<PrRow>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT i.url, i.status, json_extract(i.raw_json, '$.draft'),
                        coalesce(json_extract(i.raw_json, '$.merged'), 0) = 1 OR json_extract(i.raw_json, '$.pull_request.merged_at') IS NOT NULL OR json_extract(i.raw_json, '$.merged_at') IS NOT NULL FROM items i JOIN sources s ON s.id = i.source_id
                 WHERE s.kind = 'github' AND i.kind = 'pr'
                   AND ((?1 IS NOT NULL AND json_extract(i.raw_json, '$.head.ref') = ?1) OR (?2 IS NOT NULL AND instr(upper(i.title), ?2) > 0))
                 ORDER BY i.occurred_at DESC LIMIT 1",
                params![branch, key],
                |r| Ok(PrRow { url: r.get(0)?, status: r.get(1)?, draft: r.get::<_, Option<i64>>(2)? == Some(1), merged: r.get(3)? }),
            )
            .optional()?)
    }

    fn open_tasks_for(&self, key: &str) -> Result<Vec<TaskRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.title, s.kind, i.due_at, i.due_day, i.url FROM items i JOIN sources s ON s.id = i.source_id
                 WHERE i.kind = 'task' AND i.status != 'done' AND (i.external_id = ?1 OR instr(upper(i.title), ?1) > 0)",
            )?
            .query_map([key], |r| {
                Ok(TaskRow { title: r.get(0)?, source_kind: r.get(1)?, due: crate::time::Due::from_columns(r.get(2)?, r.get(3)?), url: r.get(4)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn signals(&self, source_id: i64) -> Result<Vec<AgentSignal>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT external_id, seen, coalesce(json_extract(raw_json, '$.auto_seen'), 0), coalesce(json_extract(raw_json, '$.agent.session'), '')
                   FROM items WHERE source_id = ?1",
            )?
            .query_map([source_id], |r| {
                Ok(AgentSignal { external_id: r.get(0)?, seen: r.get::<_, i64>(1)? != 0, auto_seen: r.get::<_, i64>(2)? != 0, session: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn ticket_topic(&self, key: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT tl.topic_id FROM items i JOIN sources src ON src.id = i.source_id JOIN topic_links tl ON tl.item_id = i.id
                  WHERE src.kind = 'jira' AND i.external_id = ?1 AND tl.state IN ('confirmed', 'suggested')
                  ORDER BY tl.state = 'confirmed' DESC LIMIT 1",
                [key],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn session_topic(&self, source_id: i64, except_item: i64, session: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT tl.topic_id FROM items i JOIN topic_links tl ON tl.item_id = i.id
                  WHERE i.source_id = ?1 AND i.id != ?2 AND json_extract(i.raw_json, '$.agent.session') = ?3 AND tl.state IN ('confirmed', 'suggested')
                  ORDER BY tl.state = 'confirmed' DESC, tl.updated_at DESC LIMIT 1",
                params![source_id, except_item, session],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl AgentState for Sql<'_> {
    fn put_host(&self, id: &str, h: &HostState) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_hosts (id, seq, turn) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, turn = excluded.turn",
            params![id, h.seq as i64, h.turn.to_string()],
        )?;
        Ok(())
    }

    fn remove_host(&self, id: &str) -> Result<()> {
        self.conn().execute("DELETE FROM agent_hosts WHERE id = ?1", [id])?;
        Ok(())
    }

    fn put_group(&self, g: &Group) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_groups (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![g.id, serde_json::to_string(g).unwrap_or_default()],
        )?;
        Ok(())
    }

    fn remove_group(&self, id: &str) -> Result<()> {
        self.conn().execute("DELETE FROM agent_groups WHERE id = ?1", [id])?;
        Ok(())
    }

    fn put_session(&self, s: &Session) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_sessions (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![s.id, serde_json::to_string(s).unwrap_or_default()],
        )?;
        Ok(())
    }

    fn remove_session(&self, id: &str) -> Result<()> {
        self.conn().execute("DELETE FROM agent_sessions WHERE id = ?1", [id])?;
        self.conn().execute("DELETE FROM agent_rows WHERE thread = ?1", [format!("s:{id}")])?;
        Ok(())
    }

    fn put_row(&self, thread: &str, r: &Row) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_rows (thread, id, data) VALUES (?1, ?2, ?3) ON CONFLICT(thread, id) DO UPDATE SET data = excluded.data",
            params![thread, r.id as i64, serde_json::to_string(r).unwrap_or_default()],
        )?;
        Ok(())
    }

    fn archive(&self, a: &Archived) -> Result<()> {
        let c = self.conn();
        c.execute(
            "INSERT OR REPLACE INTO agent_archive (id, kind, source, project, group_name, group_id, archived_at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![a.id, a.kind, a.source, a.project, a.group_name, a.group_id, a.at, a.data],
        )?;
        if a.kind == "group" {
            c.execute("DELETE FROM agent_groups WHERE id = ?1", [&a.id])?;
        } else {
            c.execute("DELETE FROM agent_sessions WHERE id = ?1", [&a.id])?;
            c.execute("DELETE FROM agent_hosts WHERE id = ?1", [&a.id])?;
        }
        Ok(())
    }

    fn snooze_nudge(&self, group: &str, until: Ts) -> Result<()> {
        self.conn().execute("INSERT INTO agent_nudges (grp, until) VALUES (?1, ?2) ON CONFLICT(grp) DO UPDATE SET until = excluded.until", params![group, until])?;
        Ok(())
    }

    fn color(&self, group: &str, name: &str, palette_len: usize) -> Result<usize> {
        let c = self.conn();
        if let Some(i) = c.query_row("SELECT idx FROM agent_colors WHERE grp = ?1 AND name = ?2", params![group, name], |r| r.get::<_, i64>(0)).optional()? {
            return Ok(i as usize);
        }
        let used: Vec<i64> = c.prepare_cached("SELECT idx FROM agent_colors WHERE grp = ?1 ORDER BY rowid")?.query_map([group], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let n = palette_len.max(1);
        let idx = (0..n).find(|i| !used.contains(&(*i as i64))).unwrap_or(used.len() % n);
        c.execute("INSERT INTO agent_colors (grp, name, idx) VALUES (?1, ?2, ?3)", params![group, name, idx as i64])?;
        Ok(idx)
    }

    fn auto_seen(&self, source_id: i64, external_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET seen = 1, raw_json = json_set(raw_json, '$.auto_seen', 1) WHERE source_id = ?1 AND external_id = ?2",
            params![source_id, external_id],
        )?;
        Ok(())
    }
}

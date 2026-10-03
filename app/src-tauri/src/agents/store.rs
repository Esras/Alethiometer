//! In-memory state of the Agents tab, with a revision counter the UI polls
//! against. Local groups, sessions and transcripts are written to SQLite by
//! `flush` (once a second from `agents::start`), not on every streamed chunk,
//! so a busy agent doesn't contend for the one database connection.
//! Scion state isn't stored: it comes back from the Hub.

use super::model::{Group, Request, Row, Session};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A local agent's place in the agent host's log, and its turn state.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostState {
    pub seq: u64,
    pub turn: serde_json::Value,
}
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub struct Store {
    inner: Mutex<Inner>,
    /// Held while an agent-host event is applied (its rows and the log
    /// position move together), and while `flush` takes its snapshot, so a
    /// crash never stores rows from an event without its position or the
    /// reverse: the next run then neither repeats nor skips it.
    gate: Mutex<()>,
}

#[derive(Default)]
pub struct Inner {
    pub rev: u64,
    /// Revision of the last change to the tree (groups, sessions, requests).
    pub tree_rev: u64,
    pub groups: BTreeMap<String, Group>,
    pub sessions: BTreeMap<String, Session>,
    pub threads: HashMap<String, Thread>,
    pub requests: BTreeMap<String, Request>,
    /// Local agents by session id (not part of the tree; no revision bump).
    pub hosts: HashMap<String, HostState>,
    dirty_hosts: HashSet<String>,
    gone_hosts: HashSet<String>,
    dirty_groups: HashSet<String>,
    dirty_sessions: HashSet<String>,
    dirty_rows: HashSet<(String, u64)>,
    gone_groups: HashSet<String>,
    gone_sessions: HashSet<String>,
}

#[derive(Default)]
pub struct Thread {
    pub rows: Vec<Row>,
    pub next_id: u64,
    /// Rows were read from the database (or the thread is new).
    pub loaded: bool,
}

/// Local state goes to the database; Scion's doesn't.
fn stored(id: &str) -> bool {
    id.starts_with("acp-") || id.starts_with("local:") || id.starts_with("s:acp-")
}

impl Store {
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn rev(&self) -> u64 {
        self.lock().rev
    }

    pub fn gate(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where to resume each local agent's log.
    pub fn host_seqs(&self) -> BTreeMap<String, u64> {
        self.lock().hosts.iter().map(|(k, h)| (k.clone(), h.seq)).collect()
    }
}

impl Inner {
    fn bump(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    fn bump_tree(&mut self) {
        let r = self.bump();
        self.tree_rev = r;
    }

    pub fn put_group(&mut self, g: Group) {
        if self.groups.get(&g.id) == Some(&g) {
            return;
        }
        if stored(&g.id) {
            self.dirty_groups.insert(g.id.clone());
        }
        self.groups.insert(g.id.clone(), g);
        self.bump_tree();
    }

    pub fn remove_group(&mut self, id: &str) {
        if self.groups.remove(id).is_some() {
            if stored(id) {
                self.gone_groups.insert(id.to_owned());
            }
            for s in self.sessions.values_mut() {
                if s.group_id.as_deref() == Some(id) {
                    s.group_id = None;
                    if stored(&s.id) {
                        self.dirty_sessions.insert(s.id.clone());
                    }
                }
            }
            self.bump_tree();
        }
    }

    pub fn put_session(&mut self, s: Session) {
        if self.sessions.get(&s.id) == Some(&s) {
            return;
        }
        if stored(&s.id) {
            self.dirty_sessions.insert(s.id.clone());
        }
        self.sessions.insert(s.id.clone(), s);
        self.bump_tree();
    }

    /// Change a session in place; a no-op change doesn't bump the revision.
    pub fn update_session(&mut self, id: &str, f: impl FnOnce(&mut Session)) -> bool {
        let Some(s) = self.sessions.get_mut(id) else { return false };
        let before = s.clone();
        f(s);
        if *s == before {
            return true;
        }
        if stored(id) {
            self.dirty_sessions.insert(id.to_owned());
        }
        self.bump_tree();
        true
    }

    pub fn remove_session(&mut self, id: &str) {
        if self.sessions.remove(id).is_some() {
            if stored(id) {
                self.gone_sessions.insert(id.to_owned());
            }
            self.threads.remove(&format!("s:{id}"));
            if self.hosts.remove(id).is_some() {
                self.gone_hosts.insert(id.to_owned());
            }
            self.requests.retain(|_, r| r.session_id != id);
            self.bump_tree();
        }
    }

    pub fn thread(&mut self, key: &str) -> &mut Thread {
        self.threads.entry(key.to_owned()).or_insert_with(|| Thread { loaded: true, ..Default::default() })
    }

    /// Append a row; returns its id.
    pub fn push_row(&mut self, key: &str, mut row: Row) -> u64 {
        let rev = self.bump();
        let t = self.thread(key);
        t.next_id = t.next_id.max(t.rows.last().map_or(0, |r| r.id)) + 1;
        row.id = t.next_id;
        row.rev = rev;
        let id = row.id;
        t.rows.push(row);
        if stored(key) {
            self.dirty_rows.insert((key.to_owned(), id));
        }
        id
    }

    /// Change a row in place (streamed text, a tool's result, a delivery state).
    pub fn update_row(&mut self, key: &str, id: u64, f: impl FnOnce(&mut Row)) -> bool {
        let rev = self.rev + 1;
        let Some(t) = self.threads.get_mut(key) else { return false };
        let Some(r) = t.rows.iter_mut().rev().find(|r| r.id == id) else { return false };
        f(r);
        r.rev = rev;
        self.rev = rev;
        if stored(key) {
            self.dirty_rows.insert((key.to_owned(), id));
        }
        true
    }

    /// The last row matching `f`, newest first.
    pub fn find_row(&self, key: &str, f: impl Fn(&Row) -> bool) -> Option<&Row> {
        self.threads.get(key)?.rows.iter().rev().find(|r| f(r))
    }

    /// Replace a thread's rows wholesale (a Scion conversation re-read from the Hub).
    /// Rows keep their ids when `ext` matches, so the UI's selection survives.
    pub fn set_rows(&mut self, key: &str, rows: Vec<Row>) {
        let rev = self.bump();
        let t = self.thread(key);
        let old: HashMap<String, Row> = t.rows.drain(..).filter(|r| !r.ext.is_empty()).map(|r| (r.ext.clone(), r)).collect();
        for mut r in rows {
            match old.get(&r.ext) {
                Some(o) => {
                    r.id = o.id;
                    r.rev = o.rev;
                    if (Row { rev: o.rev, ..r.clone() }) != *o {
                        r.rev = rev;
                    }
                }
                None => {
                    t.next_id += 1;
                    r.id = t.next_id;
                    r.rev = rev;
                }
            }
            t.rows.push(r);
        }
        t.rows.sort_by_key(|r| r.id);
        // A removed row shows as a reset: the UI drops rows it no longer gets.
    }

    /// Record how far an agent's log has been applied, with its turn state.
    pub fn set_host(&mut self, id: &str, h: HostState) {
        if self.hosts.get(id) != Some(&h) {
            self.hosts.insert(id.to_owned(), h);
            self.dirty_hosts.insert(id.to_owned());
        }
    }

    pub fn put_request(&mut self, r: Request) {
        if self.requests.get(&r.id) == Some(&r) {
            return;
        }
        self.requests.insert(r.id.clone(), r);
        self.bump_tree();
    }

    pub fn update_request(&mut self, id: &str, f: impl FnOnce(&mut Request)) {
        if let Some(r) = self.requests.get_mut(id) {
            f(r);
            self.bump_tree();
        }
    }

    pub fn remove_request(&mut self, id: &str) -> Option<Request> {
        let r = self.requests.remove(id);
        if r.is_some() {
            self.bump_tree();
        }
        r
    }
}

impl Store {
    /// Write what changed since the last flush. Takes the store's lock only to
    /// copy the changes out.
    pub fn flush(&self, conn: &Connection) -> rusqlite::Result<()> {
        // One snapshot of everything, between events, written in one transaction.
        let (hosts, gone_h, groups, sessions, rows, gone_g, gone_s) = {
            let _g = self.gate();
            let mut i = self.lock();
            let hosts: Vec<(String, HostState)> =
                i.dirty_hosts.drain().collect::<Vec<_>>().into_iter().filter_map(|id| i.hosts.get(&id).cloned().map(|h| (id, h))).collect();
            let groups: Vec<Group> = i.dirty_groups.drain().collect::<Vec<_>>().iter().filter_map(|id| i.groups.get(id).cloned()).collect();
            let sessions: Vec<Session> =
                i.dirty_sessions.drain().collect::<Vec<_>>().iter().filter_map(|id| i.sessions.get(id).cloned()).collect();
            let keys: Vec<(String, u64)> = i.dirty_rows.drain().collect();
            let rows: Vec<(String, Row)> = keys
                .into_iter()
                .filter_map(|(k, id)| i.threads.get(&k).and_then(|t| t.rows.iter().find(|r| r.id == id)).map(|r| (k, r.clone())))
                .collect();
            (hosts, std::mem::take(&mut i.gone_hosts), groups, sessions, rows, std::mem::take(&mut i.gone_groups), std::mem::take(&mut i.gone_sessions))
        };
        if hosts.is_empty() && gone_h.is_empty() && groups.is_empty() && sessions.is_empty() && rows.is_empty() && gone_g.is_empty() && gone_s.is_empty() {
            return Ok(());
        }
        let tx = conn.unchecked_transaction()?;
        for (id, h) in &hosts {
            tx.execute(
                "INSERT INTO agent_hosts (id, seq, turn) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, turn = excluded.turn",
                params![id, h.seq as i64, h.turn.to_string()],
            )?;
        }
        for id in &gone_h {
            tx.execute("DELETE FROM agent_hosts WHERE id = ?1", [id])?;
        }
        for g in &groups {
            tx.execute(
                "INSERT INTO agent_groups (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                params![g.id, serde_json::to_string(g).unwrap_or_default()],
            )?;
        }
        for s in &sessions {
            tx.execute(
                "INSERT INTO agent_sessions (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                params![s.id, serde_json::to_string(s).unwrap_or_default()],
            )?;
        }
        for (k, r) in &rows {
            tx.execute(
                "INSERT INTO agent_rows (thread, id, data) VALUES (?1, ?2, ?3) ON CONFLICT(thread, id) DO UPDATE SET data = excluded.data",
                params![k, r.id as i64, serde_json::to_string(r).unwrap_or_default()],
            )?;
        }
        for id in &gone_g {
            tx.execute("DELETE FROM agent_groups WHERE id = ?1", [id])?;
        }
        for id in &gone_s {
            tx.execute("DELETE FROM agent_sessions WHERE id = ?1", [id])?;
            tx.execute("DELETE FROM agent_rows WHERE thread = ?1", [format!("s:{id}")])?;
        }
        tx.commit()
    }

    /// Local groups and sessions at startup. Transcripts load when opened.
    pub fn load(&self, conn: &Connection) -> rusqlite::Result<()> {
        let groups: Vec<Group> = conn
            .prepare("SELECT data FROM agent_groups")?
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok().and_then(|d| serde_json::from_str(&d).ok()))
            .collect();
        let sessions: Vec<Session> = conn
            .prepare("SELECT data FROM agent_sessions")?
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok().and_then(|d| serde_json::from_str(&d).ok()))
            .collect();
        let hosts: Vec<(String, HostState)> = conn
            .prepare("SELECT id, seq, turn FROM agent_hosts")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?
            .filter_map(|r| r.ok())
            .map(|(id, seq, turn)| (id, HostState { seq: seq as u64, turn: serde_json::from_str(&turn).unwrap_or_default() }))
            .collect();
        let mut i = self.lock();
        for (id, h) in hosts {
            i.hosts.insert(id, h);
        }
        for g in groups {
            i.groups.insert(g.id.clone(), g);
        }
        for mut s in sessions {
            // Attached again once the agent host says it's still running (`acp::restore`).
            s.live = false;
            let key = format!("s:{}", s.id);
            i.threads.insert(key, Thread::default());
            i.sessions.insert(s.id.clone(), s);
        }
        i.bump_tree();
        Ok(())
    }

    /// Read a stored transcript the first time it's opened.
    pub fn ensure_loaded(&self, conn: &Connection, key: &str) -> rusqlite::Result<()> {
        if !stored(key) || self.lock().threads.get(key).is_none_or(|t| t.loaded) {
            return Ok(());
        }
        let rows: Vec<Row> = conn
            .prepare("SELECT data FROM agent_rows WHERE thread = ?1 ORDER BY id")?
            .query_map([key], |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok().and_then(|d| serde_json::from_str(&d).ok()))
            .collect();
        let mut i = self.lock();
        let rev = i.bump();
        let t = i.thread(key);
        if !t.loaded {
            let mut stored_rows: Vec<Row> = rows
                .into_iter()
                .map(|mut r| {
                    r.rev = rev;
                    r.open = false;
                    r
                })
                .collect();
            // Rows that arrived before the load (a reattach) go after the stored ones.
            let newer: Vec<Row> = t.rows.drain(..).collect();
            let mut next = stored_rows.last().map_or(0, |r| r.id);
            for mut r in newer {
                next += 1;
                r.id = next;
                stored_rows.push(r);
            }
            t.next_id = next;
            t.rows = stored_rows;
            t.loaded = true;
        }
        Ok(())
    }
}

/// Colours are assigned per group in order of creation, unique within it
/// while the palette lasts, and kept (`agent_colors`).
pub fn color_for(conn: &Connection, group: &str, name: &str, palette_len: usize) -> rusqlite::Result<usize> {
    if let Ok(i) = conn.query_row("SELECT idx FROM agent_colors WHERE grp = ?1 AND name = ?2", params![group, name], |r| r.get::<_, i64>(0)) {
        return Ok(i as usize);
    }
    let used: Vec<i64> =
        conn.prepare("SELECT idx FROM agent_colors WHERE grp = ?1 ORDER BY rowid")?.query_map([group], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let n = palette_len.max(1);
    let idx = (0..n).find(|i| !used.contains(&(*i as i64))).unwrap_or(used.len() % n);
    conn.execute("INSERT INTO agent_colors (grp, name, idx) VALUES (?1, ?2, ?3)", params![group, name, idx as i64])?;
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::{Kind, Status};

    fn conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    #[test]
    fn rows_flush_and_load_back() {
        let c = conn();
        let s = Store::default();
        {
            let mut i = s.lock();
            i.put_session(Session { id: "acp-1".into(), name: "header-fix".into(), status: Status::Running, live: true, ..Default::default() });
            let id = i.push_row("s:acp-1", Row { kind: Kind::Output, text: "Hel".into(), open: true, ..Default::default() });
            i.update_row("s:acp-1", id, |r| r.text.push_str("lo"));
            i.set_host("acp-1", HostState { seq: 7, turn: serde_json::json!({ "sid": "s1" }) });
        }
        s.flush(&c).unwrap();
        let back = Store::default();
        back.load(&c).unwrap();
        {
            let i = back.lock();
            let sess = &i.sessions["acp-1"];
            assert_eq!(sess.status, Status::Running, "kept until the agent host says otherwise");
            assert!(!sess.live);
            assert_eq!(i.hosts["acp-1"].seq, 7);
            assert_eq!(i.hosts["acp-1"].turn["sid"], "s1");
        }
        back.ensure_loaded(&c, "s:acp-1").unwrap();
        let i = back.lock();
        let rows = &i.threads["s:acp-1"].rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "Hello");
        assert!(!rows[0].open);
    }

    #[test]
    fn scion_state_is_not_stored() {
        let c = conn();
        let s = Store::default();
        s.lock().put_session(Session { id: "scion:hub:a1".into(), ..Default::default() });
        s.lock().push_row("s:scion:hub:a1", Row::default());
        s.flush(&c).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM agent_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn set_rows_keeps_ids_for_known_messages() {
        let s = Store::default();
        let row = |ext: &str, text: &str| Row { ext: ext.into(), text: text.into(), ..Default::default() };
        s.lock().set_rows("r:c1", vec![row("m1", "a"), row("m2", "b")]);
        let ids: Vec<u64> = s.lock().threads["r:c1"].rows.iter().map(|r| r.id).collect();
        let rev = s.rev();
        s.lock().set_rows("r:c1", vec![row("m1", "a"), row("m2", "b2"), row("m3", "c")]);
        let i = s.lock();
        let rows = &i.threads["r:c1"].rows;
        assert_eq!(rows[0].id, ids[0]);
        assert_eq!(rows[1].id, ids[1]);
        assert!(rows[0].rev <= rev, "unchanged row keeps its rev");
        assert!(rows[1].rev > rev, "changed row gets a new rev");
        assert_eq!(rows[2].id, ids[1] + 1);
    }

    #[test]
    fn colors_are_unique_in_a_group_and_kept() {
        let c = conn();
        assert_eq!(color_for(&c, "g", "planner", 5).unwrap(), 0);
        assert_eq!(color_for(&c, "g", "handler", 5).unwrap(), 1);
        assert_eq!(color_for(&c, "g", "planner", 5).unwrap(), 0);
        assert_eq!(color_for(&c, "other", "reviewer", 5).unwrap(), 0);
        for n in ["c", "d", "e"] {
            color_for(&c, "g", n, 5).unwrap();
        }
        // Past the palette, colours repeat.
        assert!(color_for(&c, "g", "f", 5).unwrap() < 5);
    }
}

//! In-memory state of the Agents tab, with a revision counter the UI polls
//! against. Local groups, sessions and transcripts are written to SQLite by
//! `flush` (once a second from `agents::start`), not on every streamed chunk,
//! so a busy agent doesn't contend for the one database connection.
//! Scion state isn't stored: it comes back from the Hub.

use super::model::{Group, Request, Row, Session};
pub use crate::domain::agents::HostState;
use crate::domain::ports::UnitOfWork;
use crate::error::Result;
use crate::store::Db;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub struct Store {
    inner: Mutex<Inner>,
    /// Held while an agent-host event is applied (its rows and the log
    /// position move together), and while `flush` takes its snapshot, so a
    /// crash never stores rows from an event without its position or the
    /// reverse: the next run then neither repeats nor skips it.
    ///
    /// Lock order: gate → database → turn → store (`lock`). Never take the
    /// gate while holding the database, and never take it twice on one
    /// thread: work an event leads to runs after it's released (`acp::After`).
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

/// What changed since the last flush: the keys (to mark dirty again if the
/// write fails) and their values at the time of the snapshot.
#[derive(Default)]
struct Changes {
    hosts: Vec<(String, HostState)>,
    gone_h: HashSet<String>,
    groups: Vec<Group>,
    sessions: Vec<Session>,
    row_keys: Vec<(String, u64)>,
    rows: Vec<(String, Row)>,
    gone_g: HashSet<String>,
    gone_s: HashSet<String>,
}

impl Changes {
    fn is_empty(&self) -> bool {
        self.hosts.is_empty() && self.gone_h.is_empty() && self.groups.is_empty() && self.sessions.is_empty() && self.row_keys.is_empty() && self.gone_g.is_empty() && self.gone_s.is_empty()
    }
}

impl Store {
    /// Write what changed since the last flush, in one transaction.
    ///
    /// The snapshot is taken under the gate first and the database is opened
    /// only after the gate is released: the gate is taken before the database
    /// everywhere else (an event loads its thread), so holding the database
    /// while waiting for the gate would deadlock. If the write fails,
    /// everything in it is marked dirty again.
    pub fn flush(&self, db: &Db) -> Result<()> {
        let ch = self.take_changes();
        if ch.is_empty() {
            return Ok(());
        }
        let r = db.write(|uow| write_changes(uow, &ch));
        if r.is_err() {
            let mut i = self.lock();
            i.dirty_hosts.extend(ch.hosts.into_iter().map(|(id, _)| id));
            i.gone_hosts.extend(ch.gone_h);
            i.dirty_groups.extend(ch.groups.into_iter().map(|g| g.id));
            i.dirty_sessions.extend(ch.sessions.into_iter().map(|s| s.id));
            i.dirty_rows.extend(ch.row_keys);
            i.gone_groups.extend(ch.gone_g);
            i.gone_sessions.extend(ch.gone_s);
        }
        r
    }

    /// One snapshot of everything, between events.
    fn take_changes(&self) -> Changes {
        let _g = self.gate();
        let mut i = self.lock();
        let hosts = i.dirty_hosts.drain().collect::<Vec<_>>().into_iter().filter_map(|id| i.hosts.get(&id).cloned().map(|h| (id, h))).collect();
        let groups = i.dirty_groups.drain().collect::<Vec<_>>().iter().filter_map(|id| i.groups.get(id).cloned()).collect();
        let sessions = i.dirty_sessions.drain().collect::<Vec<_>>().iter().filter_map(|id| i.sessions.get(id).cloned()).collect();
        let row_keys: Vec<(String, u64)> = i.dirty_rows.drain().collect();
        let rows = row_keys
            .iter()
            .filter_map(|(k, id)| i.threads.get(k).and_then(|t| t.rows.iter().find(|r| r.id == *id)).map(|r| (k.clone(), r.clone())))
            .collect();
        Changes {
            hosts,
            gone_h: std::mem::take(&mut i.gone_hosts),
            groups,
            sessions,
            row_keys,
            rows,
            gone_g: std::mem::take(&mut i.gone_groups),
            gone_s: std::mem::take(&mut i.gone_sessions),
        }
    }

    /// Local groups and sessions at startup. Transcripts load when opened.
    pub fn load(&self, db: &Db) -> Result<()> {
        let saved = db.read(|r| r.agents().saved())?;
        let (groups, sessions, hosts, last_ids) = (saved.groups, saved.sessions, saved.hosts, saved.last_row_ids);
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
            // Rows added before the transcript is read (a reply from Incoming, a
            // reattach) number after the stored ones, so their ids never change.
            let next_id = last_ids.get(&key).copied().unwrap_or(0);
            i.threads.insert(key, Thread { next_id, ..Default::default() });
            i.sessions.insert(s.id.clone(), s);
        }
        i.bump_tree();
        Ok(())
    }

    /// Read a stored transcript the first time it's opened.
    pub fn ensure_loaded(&self, db: &Db, key: &str) -> Result<()> {
        if !stored(key) || self.lock().threads.get(key).is_none_or(|t| t.loaded) {
            return Ok(());
        }
        let rows = db.read(|r| r.agents().transcript(key))?;
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
            // Rows that arrived before the load (a reattach, a reply) go after the
            // stored ones. Their ids were numbered past the stored ones at
            // startup (`load`) and are kept: turns and the flush refer to them.
            let newer: Vec<Row> = t.rows.drain(..).collect();
            let last_stored = stored_rows.last().map_or(0, |r| r.id);
            debug_assert!(newer.iter().all(|r| r.id > last_stored), "a row pushed before the load collides with a stored one");
            stored_rows.extend(newer);
            t.next_id = t.next_id.max(stored_rows.last().map_or(0, |r| r.id)).max(last_stored);
            t.rows = stored_rows;
            t.loaded = true;
        }
        Ok(())
    }
}

fn write_changes(uow: &dyn UnitOfWork, ch: &Changes) -> Result<()> {
    let a = uow.agents();
    for (id, h) in &ch.hosts {
        a.put_host(id, h)?;
    }
    for id in &ch.gone_h {
        a.remove_host(id)?;
    }
    for g in &ch.groups {
        a.put_group(g)?;
    }
    for s in &ch.sessions {
        a.put_session(s)?;
    }
    for (k, r) in &ch.rows {
        a.put_row(k, r)?;
    }
    for id in &ch.gone_g {
        a.remove_group(id)?;
    }
    for id in &ch.gone_s {
        a.remove_session(id)?;
    }
    Ok(())
}

/// `name`'s colour in `group` (`AgentState::color`).
pub fn color_for(db: &Db, group: &str, name: &str, palette_len: usize) -> Result<usize> {
    db.write(|uow| uow.agents().color(group, name, palette_len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::{Kind, Status};

    fn conn() -> Db {
        Db::new(crate::db::open_in_memory().unwrap()).unwrap()
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
    fn a_row_added_before_the_transcript_loads_keeps_its_id() {
        let c = conn();
        let s = Store::default();
        {
            let mut i = s.lock();
            i.put_session(Session { id: "acp-1".into(), ..Default::default() });
            for t in ["first", "second", "third"] {
                i.push_row("s:acp-1", Row { kind: Kind::You, text: t.into(), ..Default::default() });
            }
        }
        s.flush(&c).unwrap();
        // Next run: a reply goes out before anyone opens the transcript.
        let back = Store::default();
        back.load(&c).unwrap();
        let id = back.lock().push_row("s:acp-1", Row { kind: Kind::You, text: "reply".into(), ..Default::default() });
        assert_eq!(id, 4, "numbered after the stored rows");
        back.lock().update_row("s:acp-1", id, |r| r.label = "delivered".into());
        back.ensure_loaded(&c, "s:acp-1").unwrap();
        back.flush(&c).unwrap();
        let i = back.lock();
        let rows: Vec<(u64, &str, &str)> = i.threads["s:acp-1"].rows.iter().map(|r| (r.id, r.text.as_str(), r.label.as_str())).collect();
        assert_eq!(rows, vec![(1, "first", ""), (2, "second", ""), (3, "third", ""), (4, "reply", "delivered")]);
        let stored: Vec<i64> = c.legacy().prepare("SELECT id FROM agent_rows WHERE thread = 's:acp-1' ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(stored, vec![1, 2, 3, 4], "the first prompt wasn't overwritten");
    }

    #[test]
    fn scion_state_is_not_stored() {
        let c = conn();
        let s = Store::default();
        s.lock().put_session(Session { id: "scion:hub:a1".into(), ..Default::default() });
        s.lock().push_row("s:scion:hub:a1", Row::default());
        s.flush(&c).unwrap();
        let n: i64 = c.legacy().query_row("SELECT count(*) FROM agent_sessions", [], |r| r.get(0)).unwrap();
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

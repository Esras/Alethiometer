//! Split families: moving messages between an item's parts (`domain::splits`).

use super::Sql;
use crate::domain::ports::Reads;
use crate::domain::splits::{keys, Splits};
use crate::error::{msg, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Keys of messages that split children of this (source, external id) own.
fn owned_keys(conn: &Connection, source_id: i64, external_id: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.ext_key FROM items p
           JOIN items c ON c.parent_id = p.id
           JOIN item_messages m ON m.item_id = c.id
          WHERE p.source_id = ?1 AND p.external_id = ?2 AND m.ext_key IS NOT NULL",
    )?;
    let keys = stmt.query_map(params![source_id, external_id], |r| r.get(0))?.collect::<rusqlite::Result<HashSet<String>>>()?;
    Ok(keys)
}

/// (id, at, author, text, ext_key), oldest first: the order Incoming shows.
/// Replies aren't listed: they belong to (and move with) their parent.
type Row = (i64, String, String, String, Option<String>);

fn messages(conn: &Connection, item_id: i64) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare_cached("SELECT id, at, author, text, ext_key FROM item_messages WHERE item_id = ?1 AND context = 0 AND reply_to IS NULL ORDER BY at, id")?;
    let rows = stmt
        .query_map([item_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Message ids of an item, oldest first.
fn message_ids(conn: &Connection, item_id: i64) -> Result<Vec<i64>> {
    Ok(messages(conn, item_id)?.into_iter().map(|r| r.0).collect())
}

/// Rows stored before keys existed (or by seed data) get them now.
fn ensure_keys(conn: &Connection, raw: &Value, rows: &mut [Row]) -> Result<()> {
    if rows.iter().all(|r| r.4.is_some()) {
        return Ok(());
    }
    let tuples: Vec<(&str, &str, &str)> = rows.iter().map(|r| (r.1.as_str(), r.2.as_str(), r.3.as_str())).collect();
    let ks = keys(raw, &tuples);
    for (r, k) in rows.iter_mut().zip(ks) {
        if r.4.is_none() {
            conn.execute("UPDATE item_messages SET ext_key = ?2 WHERE id = ?1", params![r.0, k])?;
            r.4 = Some(k);
        }
    }
    Ok(())
}

fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        return t.to_owned();
    }
    let mut out: String = t.chars().take(n - 1).collect();
    out.push('…');
    out
}

/// Bring an item's derived fields in line with the messages it holds after
/// `removed` of them went to a child: times, count, summary, body and the
/// per-message fields in `raw_json`. With no messages left it is marked seen
/// (nothing of its own is new); it is never deleted here (see `prune`).
fn recount(conn: &Connection, item_id: i64, removed: usize) -> Result<()> {
    recount_by(conn, item_id, -(removed as i64))
}

/// `recount`, with the message count changed by `delta` (negative: moved out).
fn recount_by(conn: &Connection, item_id: i64, delta: i64) -> Result<()> {
    let rows = messages(conn, item_id)?;
    let (count, raw): (Option<i64>, String) =
        conn.query_row("SELECT msg_count, raw_json FROM items WHERE id = ?1", [item_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let count = (count.unwrap_or(0) + delta).max(rows.len() as i64);
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        conn.execute("UPDATE items SET msg_count = ?2, seen = 1 WHERE id = ?1", params![item_id, count])?;
        return Ok(());
    };
    let mut raw: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    if let Some(o) = raw.as_object_mut() {
        let ks: Vec<&str> = rows.iter().filter_map(|r| r.4.as_deref()).collect();
        if o.contains_key("msgs") && ks.len() == rows.len() {
            if o.contains_key("first_ts") {
                o.insert("first_ts".into(), json!(ks[0]));
            }
            if o.contains_key("last_ts") {
                o.insert("last_ts".into(), json!(ks[ks.len() - 1]));
            }
            o.insert("msgs".into(), json!(ks));
        }
    }
    let body = rows.iter().map(|r| format!("{}: {}", r.2, r.3)).collect::<Vec<_>>().join("\n");
    conn.execute(
        "UPDATE items SET occurred_at = ?2, ended_at = ?3, msg_count = ?4, summary = ?5, body = ?6, raw_json = ?7 WHERE id = ?1",
        params![item_id, first.1, (rows.len() > 1).then_some(&last.1), count, clip(&last.3, 140), body, raw.to_string()],
    )?;
    Ok(())
}


fn carve(conn: &Connection, item_id: i64, message_ids: &[i64], allow_all: bool) -> Result<i64> {
    let (source_id, parent_id, raw, kind): (i64, Option<i64>, String, String) = conn
        .query_row("SELECT source_id, parent_id, raw_json, kind FROM items WHERE id = ?1", [item_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional()?
        .ok_or_else(|| msg("no such item"))?;
    if kind == "email" {
        return Err(msg("an email thread moves as a whole: move the item instead"));
    }
    let raw: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let mut rows = messages(conn, item_id)?;
    let wanted: HashSet<i64> = message_ids.iter().copied().collect();
    if wanted.is_empty() {
        return Err(msg("pick at least one message to split off"));
    }
    if !wanted.iter().all(|id| rows.iter().any(|r| r.0 == *id)) {
        return Err(msg("those messages aren't in this item"));
    }
    if wanted.len() == rows.len() && !allow_all {
        return Err(msg("that's every message: move the whole item instead"));
    }
    ensure_keys(conn, &raw, &mut rows)?;

    let root = parent_id.unwrap_or(item_id);
    let root_ext: String = conn.query_row("SELECT external_id FROM items WHERE id = ?1", [root], |r| r.get(0))?;
    let mut n: i64 = conn.query_row("SELECT count(*) FROM items WHERE parent_id = ?1", [root], |r| r.get(0))?;
    let ext = loop {
        n += 1;
        let ext = format!("{root_ext}#{n}");
        let taken: bool =
            conn.query_row("SELECT EXISTS (SELECT 1 FROM items WHERE source_id = ?1 AND external_id = ?2)", params![source_id, ext], |r| r.get(0))?;
        if !taken {
            break ext;
        }
    };

    // Marking the conversation read up to the child's newest message would also
    // mark everything before it, so only the part holding the newest message
    // keeps the parent's read marker.
    let holds_newest = rows.last().is_some_and(|r| wanted.contains(&r.0));
    let mut child_raw = raw.clone();
    if let Some(o) = child_raw.as_object_mut() {
        if !holds_newest {
            o.remove("mark_ts");
        }
        o.insert("split_from".into(), json!(root_ext));
    }
    let moved = rows.iter().filter(|r| wanted.contains(&r.0)).count();
    let first = rows.iter().find(|r| wanted.contains(&r.0)).map(|r| r.1.clone()).unwrap_or_default();
    conn.execute(
        "INSERT INTO items (source_id, external_id, kind, title, url, author_person_id, occurred_at, status, msg_count, seen, raw_json, parent_id)
         SELECT source_id, ?2, kind, title, url, author_person_id, ?3, 'open', ?4, seen, ?5, ?6 FROM items WHERE id = ?1",
        params![item_id, ext, first, moved as i64, child_raw.to_string(), root],
    )?;
    let child = conn.last_insert_rowid();
    for id in &wanted {
        conn.execute("UPDATE item_messages SET item_id = ?1 WHERE id = ?2 AND item_id = ?3", params![child, id, item_id])?;
    }
    conn.execute(
        "UPDATE item_messages SET item_id = ?1 WHERE item_id = ?2
            AND reply_to IN (SELECT ext_key FROM item_messages WHERE item_id = ?1 AND reply_to IS NULL)",
        params![child, item_id],
    )?;
    conn.execute(
        "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
         SELECT ?1, topic_id, 'wrong', confidence, reason FROM topic_links WHERE item_id = ?2 AND state = 'wrong'",
        params![child, item_id],
    )?;
    recount(conn, child, 0)?;
    recount(conn, item_id, moved)?;
    Ok(child)
}

/// Move messages (and their thread replies) from one part of a split family to
/// another, both sides recounted. The read marker (`mark_ts`) is kept within
/// reach: the part receiving messages takes the giver's when it's newer, so the
/// family's newest marker survives `prune`. Returns how many moved.
fn move_messages(conn: &Connection, from: i64, message_ids: &[i64], to: i64) -> Result<usize> {
    if from == to {
        return Ok(0);
    }
    let (root, _) = Sql(conn).incoming().family(from)?;
    if Sql(conn).incoming().family(to)?.0 != root {
        return Err(msg("those items aren't parts of one signal"));
    }
    let raw_of = |id: i64| -> Result<Value> {
        let s: String = conn.query_row("SELECT raw_json FROM items WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&s).unwrap_or_else(|_| json!({})))
    };
    let from_raw = raw_of(from)?;
    let mut rows = messages(conn, from)?;
    ensure_keys(conn, &from_raw, &mut rows)?;
    let wanted: HashSet<i64> = message_ids.iter().copied().collect();
    if wanted.is_empty() || !wanted.iter().all(|id| rows.iter().any(|r| r.0 == *id)) {
        return Err(msg("those messages aren't in that part"));
    }
    for id in &wanted {
        conn.execute("UPDATE item_messages SET item_id = ?1 WHERE id = ?2 AND item_id = ?3", params![to, id, from])?;
    }
    conn.execute(
        "UPDATE item_messages SET item_id = ?1 WHERE item_id = ?2
            AND reply_to IN (SELECT ext_key FROM item_messages WHERE item_id = ?1 AND reply_to IS NULL)",
        params![to, from],
    )?;
    let ts = |v: &Value| v.get("mark_ts").and_then(Value::as_str).map(|t| (t.parse::<f64>().unwrap_or(0.0), t.to_owned()));
    if let Some((n, t)) = ts(&from_raw) {
        let mut to_raw = raw_of(to)?;
        if ts(&to_raw).is_none_or(|(m, _)| m < n) {
            if let Some(o) = to_raw.as_object_mut() {
                o.insert("mark_ts".into(), json!(t));
                conn.execute("UPDATE items SET raw_json = ?2 WHERE id = ?1", params![to, to_raw.to_string()])?;
            }
        }
    }
    let n = wanted.len();
    recount_by(conn, to, n as i64)?;
    recount_by(conn, from, -(n as i64))?;
    Ok(n)
}

/// Remove a child part the split review emptied: no messages left and no
/// todo held on it (its topic link goes with it; the messages now live in
/// another part). The root is never removed, and neither is anything another
/// table still points at (an outbox row, say). Returns whether it went.
fn prune(conn: &Connection, item_id: i64) -> Result<bool> {
    let removable: bool = conn.query_row(
        "SELECT i.parent_id IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM item_messages WHERE item_id = i.id)
            AND NOT EXISTS (SELECT 1 FROM held_todos WHERE item_id = i.id)
           FROM items i WHERE i.id = ?1",
        [item_id],
        |r| r.get(0),
    )?;
    if !removable {
        return Ok(false);
    }
    conn.execute_batch("SAVEPOINT prune")?;
    let gone = conn
        .execute("DELETE FROM topic_links WHERE item_id = ?1", [item_id])
        .and_then(|_| conn.execute("DELETE FROM items WHERE id = ?1", [item_id]));
    match gone {
        Ok(_) => {
            conn.execute_batch("RELEASE prune")?;
            Ok(true)
        }
        // Something else still refers to it (foreign keys): it stays, empty and seen.
        Err(_) => {
            conn.execute_batch("ROLLBACK TO prune; RELEASE prune")?;
            Ok(false)
        }
    }
}


impl Splits for Sql<'_> {
    fn message_ids(&self, item_id: i64) -> Result<Vec<i64>> {
        message_ids(self.conn(), item_id)
    }
    fn carve(&self, item_id: i64, message_ids: &[i64], allow_all: bool) -> Result<i64> {
        carve(self.conn(), item_id, message_ids, allow_all)
    }
    fn move_messages(&self, from: i64, message_ids: &[i64], to: i64) -> Result<usize> {
        move_messages(self.conn(), from, message_ids, to)
    }
    fn prune(&self, item_id: i64) -> Result<bool> {
        prune(self.conn(), item_id)
    }
    fn recount(&self, item_id: i64, removed: usize) -> Result<()> {
        recount(self.conn(), item_id, removed)
    }
    fn owned_keys(&self, source_id: i64, external_id: &str) -> Result<HashSet<String>> {
        owned_keys(self.conn(), source_id, external_id)
    }
}

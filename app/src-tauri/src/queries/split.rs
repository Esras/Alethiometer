//! Topic-aware splits. A signal can hold several messages that arrived together
//! (a Slack DM's unread run, a thread's new replies) without being about one
//! thing. A split moves some of them to a child item that gets its own topic
//! link; the parent keeps the rest.
//!
//! - A child is an ordinary item: same source, `external_id` `<root ext>#<n>`,
//!   `parent_id` = the root item (families stay flat: splitting a child again
//!   hangs the new child off the same root). It shows in Incoming and in a
//!   topic's chronology like any other item.
//! - Messages are moved, not copied (`item_messages.item_id` changes).
//!   A thread reply (`reply_to` = its parent's key) moves with its parent.
//! - Each message has a key (`item_messages.ext_key`): the connector's id when
//!   `raw_json.msgs` lists one per message (Slack's ts), else a content hash.
//!   `sync::apply` skips keys a child owns, so the next sync of the parent never
//!   re-adds a moved message; new messages still land on the parent.
//! - A child inherits the parent's `wrong` links (the user already said the
//!   whole run isn't about those topics) and its source fields (`raw_json`),
//!   except `mark_ts`, which it keeps only when it holds the parent's newest
//!   message. Marking read is gated by family in `incoming::mark_read_remote`.
//!
//! Nothing is deleted: a split only re-parents message rows.

use crate::error::{msg, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashSet;

/// FNV-1a, 64 bit: stable across builds (std's hasher isn't promised to be).
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn content_key(at: &str, author: &str, text: &str) -> String {
    format!("h:{:016x}", fnv(&format!("{at}\u{1f}{author}\u{1f}{text}")))
}

/// One key per message, in order: `raw_json.msgs` when it lists exactly one
/// string per message, otherwise a hash of time, author and text.
pub fn keys(raw: &Value, msgs: &[(&str, &str, &str)]) -> Vec<String> {
    let ids: Option<Vec<&str>> = raw.get("msgs").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect());
    match ids {
        Some(ids) if ids.len() == msgs.len() => ids.into_iter().map(str::to_owned).collect(),
        _ => msgs.iter().map(|(at, author, text)| content_key(at, author, text)).collect(),
    }
}

/// Keys of messages that split children of this (source, external id) own.
pub fn owned_keys(conn: &Connection, source_id: i64, external_id: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.ext_key FROM items p
           JOIN items c ON c.parent_id = p.id
           JOIN item_messages m ON m.item_id = c.id
          WHERE p.source_id = ?1 AND p.external_id = ?2 AND m.ext_key IS NOT NULL",
    )?;
    let keys = stmt.query_map(params![source_id, external_id], |r| r.get(0))?.collect::<rusqlite::Result<HashSet<String>>>()?;
    Ok(keys)
}

/// The item holding message `key` of `channel` as its own (not context, not
/// a reply): where that message's thread replies go.
pub fn holder(conn: &Connection, source_id: i64, channel: Option<&str>, key: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT m.item_id FROM item_messages m JOIN items i ON i.id = m.item_id
              WHERE m.ext_key = ?2 AND m.context = 0 AND m.reply_to IS NULL AND i.source_id = ?1
                AND json_extract(i.raw_json, '$.channel') IS ?3
              ORDER BY m.id DESC LIMIT 1",
            params![source_id, key, channel],
            |r| r.get(0),
        )
        .optional()?)
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
pub fn message_ids(conn: &Connection, item_id: i64) -> Result<Vec<i64>> {
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
pub fn recount(conn: &Connection, item_id: i64, removed: usize) -> Result<()> {
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

/// Move `message_ids` (a subset of the item's messages) to a new child item.
/// Returns the child's id. The child starts with the parent's `seen` state and
/// no topic link except inherited `wrong` ones; the caller links it. Run inside
/// a transaction.
pub fn split_off(conn: &Connection, item_id: i64, message_ids: &[i64]) -> Result<i64> {
    carve(conn, item_id, message_ids, false)
}

/// `split_off`, optionally taking every message (the split review moves whole
/// parts; the item left empty is marked seen by `recount`).
pub fn carve(conn: &Connection, item_id: i64, message_ids: &[i64], allow_all: bool) -> Result<i64> {
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

/// The root of an item's split family and every part (root first, then
/// children in the order they were made).
pub fn family(conn: &Connection, item_id: i64) -> Result<(i64, Vec<i64>)> {
    let parent: Option<i64> = conn
        .query_row("SELECT parent_id FROM items WHERE id = ?1", [item_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| msg("no such item"))?;
    let root = parent.unwrap_or(item_id);
    let parts = conn
        .prepare_cached("SELECT id FROM items WHERE id = ?1 OR parent_id = ?1 ORDER BY id = ?1 DESC, id")?
        .query_map([root], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok((root, parts))
}

/// Move messages (and their thread replies) from one part of a split family to
/// another, both sides recounted. The read marker (`mark_ts`) is kept within
/// reach: the part receiving messages takes the giver's when it's newer, so the
/// family's newest marker survives `prune`. Returns how many moved.
pub fn move_messages(conn: &Connection, from: i64, message_ids: &[i64], to: i64) -> Result<usize> {
    if from == to {
        return Ok(0);
    }
    let (root, _) = family(conn, from)?;
    if family(conn, to)?.0 != root {
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
pub fn prune(conn: &Connection, item_id: i64) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{apply, load_source, register_sources, Batch, ItemUpsert, Message};

    fn slack() -> (Connection, crate::sync::Source) {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = crate::settings::SourceConfig {
            kind: "slack".into(),
            tag: "work".into(),
            name: "Slack".into(),
            enabled: true,
            poll_secs: 300,
            config: json!({}),
        };
        register_sources(&conn, &[cfg]).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM sources", [], |r| r.get(0)).unwrap();
        let src = load_source(&conn, id).unwrap();
        (conn, src)
    }

    /// A DM's unread run as the Slack connector emits it.
    fn run(ts: &[&str], bump: bool) -> ItemUpsert {
        let messages: Vec<Message> = ts
            .iter()
            .map(|t| Message { at: format!("2026-09-27T09:{t}:00"), author: "Priya".into(), text: format!("msg {t}"), ..Default::default() })
            .collect();
        ItemUpsert {
            external_id: "D1/100.01".into(),
            kind: "thread".into(),
            title: "DM · Priya".into(),
            occurred_at: messages[0].at.clone(),
            ended_at: Some(messages[messages.len() - 1].at.clone()),
            status: "open".into(),
            msg_count: Some(ts.len() as i64),
            raw_json: json!({ "channel": "D1", "first_ts": ts[0], "last_ts": ts[ts.len() - 1], "mark_ts": ts[ts.len() - 1], "msgs": ts }),
            messages,
            bump,
            ..Default::default()
        }
    }

    fn texts(conn: &Connection, item: i64) -> Vec<String> {
        messages(conn, item).unwrap().into_iter().map(|r| r.3).collect()
    }

    fn item_id(conn: &Connection, ext: &str) -> i64 {
        conn.query_row("SELECT id FROM items WHERE external_id = ?1", [ext], |r| r.get(0)).unwrap()
    }

    fn raw(conn: &Connection, item: i64) -> Value {
        let s: String = conn.query_row("SELECT raw_json FROM items WHERE id = ?1", [item], |r| r.get(0)).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn split_moves_messages_and_fixes_both_sides() {
        let (conn, src) = slack();
        apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12", "13"], true)], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        let ids = message_ids(&conn, parent).unwrap();
        let topic: i64 = {
            conn.execute("INSERT INTO topics (name, category) VALUES ('X', 'work')", []).unwrap();
            conn.last_insert_rowid()
        };
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'wrong')", params![parent, topic]).unwrap();
        let child = split_off(&conn, parent, &[ids[0], ids[1]]).unwrap();

        assert_eq!(texts(&conn, child), ["msg 10", "msg 11"]);
        assert_eq!(texts(&conn, parent), ["msg 12", "msg 13"]);
        let total: i64 = conn.query_row("SELECT count(*) FROM item_messages", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 4, "moved, not copied");
        let (ext, pid, count, at, seen): (String, i64, i64, String, i64) = conn
            .query_row("SELECT external_id, parent_id, msg_count, occurred_at, seen FROM items WHERE id = ?1", [child], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!((ext.as_str(), pid, count, at.as_str(), seen), ("D1/100.01#1", parent, 2, "2026-09-27T09:10:00", 0));
        let c = raw(&conn, child);
        assert_eq!(c["msgs"], json!(["10", "11"]));
        assert_eq!(c["channel"], "D1");
        assert!(c.get("mark_ts").is_none(), "older part never marks the conversation read");
        let p = raw(&conn, parent);
        assert_eq!(p["msgs"], json!(["12", "13"]));
        assert_eq!(p["first_ts"], "12");
        let (pcount, pat): (i64, String) =
            conn.query_row("SELECT msg_count, occurred_at FROM items WHERE id = ?1", [parent], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((pcount, pat.as_str()), (2, "2026-09-27T09:12:00"));
        let wrong: i64 =
            conn.query_row("SELECT count(*) FROM topic_links WHERE item_id = ?1 AND state = 'wrong'", [child], |r| r.get(0)).unwrap();
        assert_eq!(wrong, 1, "child inherits wrong-topic exclusions");

        // The newest part keeps the read marker; a second split off the parent
        // hangs off the same root with the next number.
        let ids = message_ids(&conn, parent).unwrap();
        let newest = split_off(&conn, parent, &[ids[1]]).unwrap();
        let n = raw(&conn, newest);
        assert_eq!(n["mark_ts"], "13");
        let ext: String = conn.query_row("SELECT external_id FROM items WHERE id = ?1", [newest], |r| r.get(0)).unwrap();
        assert_eq!(ext, "D1/100.01#2");
        // Splitting a child again stays in the flat family.
        assert!(split_off(&conn, child, &message_ids(&conn, child).unwrap()).is_err(), "all messages is a move");
        let ids = message_ids(&conn, child).unwrap();
        let grand = split_off(&conn, child, &[ids[1]]).unwrap();
        let (gpid, gext): (i64, String) =
            conn.query_row("SELECT parent_id, external_id FROM items WHERE id = ?1", [grand], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((gpid, gext.as_str()), (parent, "D1/100.01#3"));
    }

    #[test]
    fn bad_selections_are_refused() {
        let (conn, src) = slack();
        apply(&conn, &src, &Batch { items: vec![run(&["10", "11"], true)], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        assert!(split_off(&conn, parent, &[]).is_err());
        assert!(split_off(&conn, parent, &[9999]).is_err());
        assert!(split_off(&conn, 9999, &[1]).is_err());
    }

    #[test]
    fn next_sync_does_not_readd_moved_messages() {
        let (conn, src) = slack();
        apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], true)], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        let ids = message_ids(&conn, parent).unwrap();
        let child = split_off(&conn, parent, &[ids[0]]).unwrap();
        conn.execute("UPDATE items SET seen = 1", []).unwrap();

        // Same run again, nothing new: moved message stays with the child, no bump.
        assert_eq!(apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], false)], ..Default::default() }).unwrap(), 0);
        assert_eq!(texts(&conn, parent), ["msg 11", "msg 12"]);
        assert_eq!(texts(&conn, child), ["msg 10"]);
        let (count, first): (i64, String) =
            conn.query_row("SELECT msg_count, json_extract(raw_json, '$.first_ts') FROM items WHERE id = ?1", [parent], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((count, first.as_str()), (2, "11"));

        // A new message lands on the parent and makes it new again.
        assert_eq!(apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12", "14"], true)], ..Default::default() }).unwrap(), 1);
        assert_eq!(texts(&conn, parent), ["msg 11", "msg 12", "msg 14"]);
        assert_eq!(texts(&conn, child), ["msg 10"]);

        // Only moved messages left in the run (the rest read in Slack): the
        // parent keeps nothing of its own, stops being new, isn't deleted, and
        // the connector's bump doesn't resurface it.
        assert_eq!(apply(&conn, &src, &Batch { items: vec![run(&["10"], true)], ..Default::default() }).unwrap(), 0);
        let (n, seen): (i64, i64) = conn
            .query_row("SELECT (SELECT count(*) FROM item_messages WHERE item_id = ?1), seen FROM items WHERE id = ?1", [parent], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, seen), (0, 1));
        assert_eq!(texts(&conn, child), ["msg 10"]);
    }

    /// Replies to message `parent` of D1, as the Slack connector sends a thread.
    fn thread(parent: &str, ts: &[&str], bump: bool) -> ItemUpsert {
        let all: Vec<&str> = std::iter::once(parent).chain(ts.iter().copied()).collect();
        let mut it = run(&all, bump);
        it.external_id = format!("D1/thread/{parent}");
        it.messages[0].context = true;
        it.raw_json = json!({ "channel": "D1", "thread_ts": parent, "msgs": all });
        it.attach_to = Some(parent.into());
        it
    }

    #[test]
    fn a_thread_is_stored_under_its_parent_and_moves_with_it() {
        let (conn, src) = slack();
        apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], true), thread("11", &["20", "21"], true)], ..Default::default() }).unwrap();
        let dm = item_id(&conn, "D1/100.01");
        let n: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "no thread item");
        let under = |item: i64| -> Vec<String> {
            conn.prepare("SELECT text FROM item_messages WHERE item_id = ?1 AND reply_to = '11' ORDER BY at")
                .unwrap()
                .query_map([item], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(under(dm), ["msg 20", "msg 21"]);
        assert_eq!(texts(&conn, dm), ["msg 10", "msg 11", "msg 12"], "replies aren't top-level messages");

        // The run re-synced keeps the replies; a new reply after triage brings it back.
        conn.execute("UPDATE items SET seen = 1", []).unwrap();
        apply(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], false)], ..Default::default() }).unwrap();
        assert_eq!(apply(&conn, &src, &Batch { items: vec![thread("11", &["20", "21", "22"], true)], ..Default::default() }).unwrap(), 1);
        assert_eq!(under(dm), ["msg 20", "msg 21", "msg 22"]);
        let seen: i64 = conn.query_row("SELECT seen FROM items WHERE id = ?1", [dm], |r| r.get(0)).unwrap();
        assert_eq!(seen, 0);

        // Splitting the parent off takes its replies; the next reply follows it there.
        let ids = message_ids(&conn, dm).unwrap();
        let child = split_off(&conn, dm, &[ids[1]]).unwrap();
        assert!(under(dm).is_empty());
        apply(&conn, &src, &Batch { items: vec![thread("11", &["20", "21", "22", "23"], true)], ..Default::default() }).unwrap();
        assert_eq!(under(child), ["msg 20", "msg 21", "msg 22", "msg 23"]);

        // Parent never captured: the thread is its own item, and stays one.
        apply(&conn, &src, &Batch { items: vec![thread("05", &["30"], true)], ..Default::default() }).unwrap();
        let orphan = item_id(&conn, "D1/thread/05");
        apply(&conn, &src, &Batch { items: vec![run(&["05"], true), thread("05", &["30", "31"], true)], ..Default::default() }).unwrap();
        assert_eq!(texts(&conn, orphan), ["msg 30", "msg 31"]);
    }

    #[test]
    fn keyless_sources_use_content_keys() {
        let (conn, src) = slack();
        let mut it = run(&["10", "11"], true);
        it.raw_json = json!({});
        apply(&conn, &src, &Batch { items: vec![it.clone()], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        let ids = message_ids(&conn, parent).unwrap();
        split_off(&conn, parent, &[ids[0]]).unwrap();
        it.bump = false;
        apply(&conn, &src, &Batch { items: vec![it], ..Default::default() }).unwrap();
        assert_eq!(texts(&conn, parent), ["msg 11"]);
    }
}

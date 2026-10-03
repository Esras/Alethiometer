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

use crate::domain::ports::UnitOfWork;
use crate::error::Result;
use rusqlite::Connection;
use std::collections::HashSet;

pub use crate::domain::splits::{content_key, keys};

// Over a connection code not moved to units of work yet holds (`store::over`).

pub fn owned_keys(conn: &Connection, source_id: i64, external_id: &str) -> Result<HashSet<String>> {
    crate::store::over(conn).splits().owned_keys(source_id, external_id)
}

pub fn holder(conn: &Connection, source_id: i64, channel: Option<&str>, key: &str) -> Result<Option<i64>> {
    crate::store::over(conn).splits().holder(source_id, channel, key)
}

/// Message ids of an item, oldest first.
pub fn message_ids(conn: &Connection, item_id: i64) -> Result<Vec<i64>> {
    crate::store::over(conn).splits().message_ids(item_id)
}

pub fn recount(conn: &Connection, item_id: i64, removed: usize) -> Result<()> {
    crate::store::over(conn).splits().recount(item_id, removed)
}

/// Move `message_ids` (a subset of the item's messages) to a new child item.
/// Returns the child's id; the caller links it. Run inside a transaction.
pub fn split_off(conn: &Connection, item_id: i64, message_ids: &[i64]) -> Result<i64> {
    crate::store::over(conn).splits().carve(item_id, message_ids, false)
}

pub fn carve(conn: &Connection, item_id: i64, message_ids: &[i64], allow_all: bool) -> Result<i64> {
    crate::store::over(conn).splits().carve(item_id, message_ids, allow_all)
}

pub fn family(conn: &Connection, item_id: i64) -> Result<(i64, Vec<i64>)> {
    use crate::domain::ports::Reads;
    crate::store::over(conn).incoming().family(item_id)
}

pub fn move_messages(conn: &Connection, from: i64, message_ids: &[i64], to: i64) -> Result<usize> {
    crate::store::over(conn).splits().move_messages(from, message_ids, to)
}

pub fn prune(conn: &Connection, item_id: i64) -> Result<bool> {
    crate::store::over(conn).splits().prune(item_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::{json, Value};
    use crate::sync::{apply_now, load_source, register_sources, Batch, ItemUpsert, Message};

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
            .map(|t| Message { at: crate::time::t(&format!("2026-09-27T09:{t}:00")), author: "Priya".into(), text: format!("msg {t}"), ..Default::default() })
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
        conn.prepare("SELECT text FROM item_messages WHERE item_id = ?1 AND context = 0 AND reply_to IS NULL ORDER BY at, id")
            .unwrap()
            .query_map([item], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
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
        apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12", "13"], true)], ..Default::default() }).unwrap();
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
        assert_eq!((ext.as_str(), pid, count, at.as_str(), seen), ("D1/100.01#1", parent, 2, "2026-09-27T09:10:00.000Z", 0));
        let c = raw(&conn, child);
        assert_eq!(c["msgs"], json!(["10", "11"]));
        assert_eq!(c["channel"], "D1");
        assert!(c.get("mark_ts").is_none(), "older part never marks the conversation read");
        let p = raw(&conn, parent);
        assert_eq!(p["msgs"], json!(["12", "13"]));
        assert_eq!(p["first_ts"], "12");
        let (pcount, pat): (i64, String) =
            conn.query_row("SELECT msg_count, occurred_at FROM items WHERE id = ?1", [parent], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((pcount, pat.as_str()), (2, "2026-09-27T09:12:00.000Z"));
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
        apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11"], true)], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        assert!(split_off(&conn, parent, &[]).is_err());
        assert!(split_off(&conn, parent, &[9999]).is_err());
        assert!(split_off(&conn, 9999, &[1]).is_err());
    }

    #[test]
    fn next_sync_does_not_readd_moved_messages() {
        let (conn, src) = slack();
        apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], true)], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        let ids = message_ids(&conn, parent).unwrap();
        let child = split_off(&conn, parent, &[ids[0]]).unwrap();
        conn.execute("UPDATE items SET seen = 1", []).unwrap();

        // Same run again, nothing new: moved message stays with the child, no bump.
        assert_eq!(apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], false)], ..Default::default() }).unwrap(), 0);
        assert_eq!(texts(&conn, parent), ["msg 11", "msg 12"]);
        assert_eq!(texts(&conn, child), ["msg 10"]);
        let (count, first): (i64, String) =
            conn.query_row("SELECT msg_count, json_extract(raw_json, '$.first_ts') FROM items WHERE id = ?1", [parent], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((count, first.as_str()), (2, "11"));

        // A new message lands on the parent and makes it new again.
        assert_eq!(apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12", "14"], true)], ..Default::default() }).unwrap(), 1);
        assert_eq!(texts(&conn, parent), ["msg 11", "msg 12", "msg 14"]);
        assert_eq!(texts(&conn, child), ["msg 10"]);

        // Only moved messages left in the run (the rest read in Slack): the
        // parent keeps nothing of its own, stops being new, isn't deleted, and
        // the connector's bump doesn't resurface it.
        assert_eq!(apply_now(&conn, &src, &Batch { items: vec![run(&["10"], true)], ..Default::default() }).unwrap(), 0);
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
        apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], true), thread("11", &["20", "21"], true)], ..Default::default() }).unwrap();
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
        apply_now(&conn, &src, &Batch { items: vec![run(&["10", "11", "12"], false)], ..Default::default() }).unwrap();
        assert_eq!(apply_now(&conn, &src, &Batch { items: vec![thread("11", &["20", "21", "22"], true)], ..Default::default() }).unwrap(), 1);
        assert_eq!(under(dm), ["msg 20", "msg 21", "msg 22"]);
        let seen: i64 = conn.query_row("SELECT seen FROM items WHERE id = ?1", [dm], |r| r.get(0)).unwrap();
        assert_eq!(seen, 0);

        // Splitting the parent off takes its replies; the next reply follows it there.
        let ids = message_ids(&conn, dm).unwrap();
        let child = split_off(&conn, dm, &[ids[1]]).unwrap();
        assert!(under(dm).is_empty());
        apply_now(&conn, &src, &Batch { items: vec![thread("11", &["20", "21", "22", "23"], true)], ..Default::default() }).unwrap();
        assert_eq!(under(child), ["msg 20", "msg 21", "msg 22", "msg 23"]);

        // Parent never captured: the thread is its own item, and stays one.
        apply_now(&conn, &src, &Batch { items: vec![thread("05", &["30"], true)], ..Default::default() }).unwrap();
        let orphan = item_id(&conn, "D1/thread/05");
        apply_now(&conn, &src, &Batch { items: vec![run(&["05"], true), thread("05", &["30", "31"], true)], ..Default::default() }).unwrap();
        assert_eq!(texts(&conn, orphan), ["msg 30", "msg 31"]);
    }

    #[test]
    fn keyless_sources_use_content_keys() {
        let (conn, src) = slack();
        let mut it = run(&["10", "11"], true);
        it.raw_json = json!({});
        apply_now(&conn, &src, &Batch { items: vec![it.clone()], ..Default::default() }).unwrap();
        let parent = item_id(&conn, "D1/100.01");
        let ids = message_ids(&conn, parent).unwrap();
        split_off(&conn, parent, &[ids[0]]).unwrap();
        it.bump = false;
        apply_now(&conn, &src, &Batch { items: vec![it], ..Default::default() }).unwrap();
        assert_eq!(texts(&conn, parent), ["msg 11"]);
    }
}

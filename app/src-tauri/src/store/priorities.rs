//! Priorities: each open todo's facts, for scoring.

use super::Sql;
use crate::domain::priorities::{Author, PriorityViews, TodoFacts};
use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::{Date, Due, Ts};
use rusqlite::params;

impl PriorityViews for Sql<'_> {
    fn facts(&self) -> Result<Vec<TodoFacts>> {
        Ok(self
            .conn()
            .prepare_cached(
                "WITH todo AS (
                   SELECT i.*, s.kind AS skind,
                          CASE s.kind WHEN 'jira' THEN i.id WHEN 'notes' THEN json_extract(i.raw_json, '$.from_item') END AS thread,
                          (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong'
                            ORDER BY tl.state = 'confirmed' DESC LIMIT 1) AS topic_id
                     FROM items i JOIN sources s ON s.id = i.source_id
                    WHERE s.tag = 'work' AND i.owner = 'you' AND i.status = 'open'
                      AND ((s.kind IN ('notes', 'obsidian') AND i.kind = 'task') OR (s.kind = 'jira' AND i.kind = 'issue'))
                 ),
                 last AS (
                   -- The newest message on each thread by anyone but a bot.
                   SELECT t.id AS item_id, m.at, m.author, m.author_id, ts.kind AS kind
                     FROM todo t
                     JOIN items th ON th.id = t.thread
                     JOIN sources ts ON ts.id = th.source_id
                     JOIN item_messages m ON m.id = (
                       SELECT m2.id FROM item_messages m2
                        WHERE m2.item_id = th.id AND m2.context = 0
                          AND NOT EXISTS (SELECT 1 FROM account_profiles ap
                                           WHERE ap.kind = ts.kind AND ap.account = m2.author_id AND ap.is_bot = 1)
                        ORDER BY m2.at DESC, m2.id DESC LIMIT 1)
                 )
                 SELECT t.id, t.skind, t.title, t.url, t.raw_json, t.summary, t.due_at, t.due_day, t.sched_day,
                        e.minutes, e.by, tp.id, tp.name, tp.category, tp.priority,
                        CASE WHEN t.skind = 'jira' AND json_valid(t.raw_json) THEN
                          (SELECT count(*) FROM json_each(t.raw_json, '$.fields.issuelinks') l
                            WHERE json_extract(l.value, '$.type.outward') = 'blocks'
                              AND json_extract(l.value, '$.outwardIssue.key') IS NOT NULL
                              AND coalesce(json_extract(l.value, '$.outwardIssue.fields.status.statusCategory.key'), '') != 'done')
                        ELSE 0 END,
                        (SELECT count(*) FROM topic_people x WHERE x.topic_id = tp.id),
                        (SELECT count(DISTINCT li.source_id) FROM topic_links x JOIN items li ON li.id = x.item_id
                          WHERE x.topic_id = tp.id AND x.state = 'confirmed'),
                        coalesce(t.source_created_at, t.created_at),
                        max(t.updated_at, coalesce((SELECT max(x.updated_at) FROM topic_links x WHERE x.item_id = t.id), t.updated_at)),
                        last.at, last.kind, last.author_id, last.author,
                        asker.kind, ah.handle, ap.display_name,
                        (SELECT json_group_array(tag) FROM (
                           SELECT tag FROM item_tags WHERE item_id = t.id AND state = 'confirmed'
                           UNION SELECT tag FROM topic_tags WHERE topic_id = tp.id AND state = 'confirmed'))
                   FROM todo t
                   LEFT JOIN todo_estimates e ON e.item_id = t.id
                   LEFT JOIN topics tp ON tp.id = t.topic_id
                   LEFT JOIN last ON last.item_id = t.id
                   -- Who asked: the signal a local todo came from, or the Jira issue itself (its reporter).
                   LEFT JOIN (SELECT ai.id, ai.author_person_id, asrc.kind FROM items ai JOIN sources asrc ON asrc.id = ai.source_id) asker
                          ON asker.id = CASE t.skind WHEN 'notes' THEN json_extract(t.raw_json, '$.from_item') WHEN 'jira' THEN t.id END
                   LEFT JOIN people ap ON ap.id = asker.author_person_id
                   LEFT JOIN person_handles ah ON ah.person_id = asker.author_person_id AND ah.source_kind = asker.kind
                  ORDER BY t.id",
            )?
            .query_map([], |r| {
                let topic = match r.get::<_, Option<i64>>(11)? {
                    Some(id) => Some(TopicRef { id, name: r.get(12)?, category: r.get(13)? }),
                    None => None,
                };
                let estimate = match (r.get::<_, Option<i64>>(9)?, r.get::<_, Option<String>>(10)?) {
                    (Some(m), Some(by)) => Some((m, by)),
                    _ => None,
                };
                let last_author = match (r.get::<_, Option<String>>(21)?, r.get::<_, Option<String>>(23)?) {
                    (Some(source_kind), Some(name)) => Some(Author { source_kind, author_id: r.get(22)?, name }),
                    _ => None,
                };
                let asker = match (r.get::<_, Option<String>>(24)?, r.get::<_, Option<String>>(26)?) {
                    (Some(source_kind), Some(name)) => Some(Author { source_kind, author_id: r.get(25)?, name }),
                    _ => None,
                };
                Ok(TodoFacts {
                    item_id: r.get(0)?,
                    source_kind: r.get(1)?,
                    title: r.get(2)?,
                    url: r.get(3)?,
                    raw_json: r.get(4)?,
                    summary: r.get(5)?,
                    due: Due::from_columns(r.get(6)?, r.get::<_, Option<Date>>(7)?),
                    sched: r.get(8)?,
                    estimate,
                    topic,
                    topic_priority: r.get(14)?,
                    blocks_open: r.get(15)?,
                    topic_people: r.get(16)?,
                    topic_sources: r.get(17)?,
                    created: r.get(18)?,
                    changed: r.get(19)?,
                    last_signal: r.get(20)?,
                    last_author,
                    asker,
                    tags: serde_json::from_str(&r.get::<_, String>(27)?).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn next_meetings(&self, now: Ts, until: Ts) -> Result<Vec<(i64, Ts, Ts)>> {
        let rows: Vec<(i64, Ts, Ts)> = self
            .conn()
            .prepare_cached(
                "SELECT tl.topic_id, e.start, e.end FROM events e
                   JOIN topic_links tl ON tl.item_id = e.item_id AND tl.state != 'wrong'
                  WHERE e.is_life_busy = 0 AND e.end > ?1 AND e.start < ?2
                  ORDER BY tl.topic_id, e.start",
            )?
            .query_map(params![now, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out: Vec<(i64, Ts, Ts)> = vec![];
        for row in rows {
            if out.last().is_none_or(|l| l.0 != row.0) {
                out.push(row);
            }
        }
        Ok(out)
    }
}

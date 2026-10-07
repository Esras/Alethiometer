//! Answering from Alethiometer: invites, their overlaps, and the answers queued.

use super::Sql;
use crate::domain::respond::{AnswerRow, InviteRow, OverlapRow, RespondViews};
use crate::error::Result;
use rusqlite::{params, Row};

/// A JSON array of ids, for `json_each`.
fn id_list(ids: &[i64]) -> String {
    serde_json::to_string(ids).expect("ids serialize")
}

fn answer(r: &Row) -> rusqlite::Result<AnswerRow> {
    Ok(AnswerRow {
        item_id: r.get(0)?,
        outbox_id: r.get(1)?,
        state: r.get(2)?,
        payload_json: r.get(3)?,
        error: r.get(4)?,
        created_at: r.get(5)?,
        confirmed_at: r.get(6)?,
    })
}

const ANSWER: &str = "k.item_id, o.id, o.state, o.payload_json, o.error, o.created_at, o.confirmed_at";

impl RespondViews for Sql<'_> {
    fn invites(&self, item_ids: &[i64]) -> Result<Vec<InviteRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                // `self` is JSON true, which json_extract reads as 1.
                "SELECT i.id, json_extract(a.value, '$.responseStatus'),
                        coalesce(json_extract(i.raw_json, '$.recurringEventId'), i.external_id), i.occurred_at, i.ended_at, i.url
                   FROM items i JOIN sources s ON s.id = i.source_id, json_each(i.raw_json, '$.attendees') a
                  WHERE i.id IN (SELECT value FROM json_each(?1)) AND s.kind = 'gcal' AND i.kind = 'event'
                    AND json_extract(a.value, '$.self') = 1 AND json_extract(a.value, '$.responseStatus') IS NOT NULL",
            )?
            .query_map([id_list(item_ids)], |r| {
                Ok(InviteRow { item_id: r.get(0)?, response: r.get(1)?, event_id: r.get(2)?, start: r.get(3)?, end: r.get(4)?, url: r.get(5)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn overlaps(&self, item_ids: &[i64]) -> Result<Vec<OverlapRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT me.item_id, CASE WHEN o.is_life_busy = 1 THEN NULL ELSE coalesce(o.title, '(No title)') END, o.start, o.end
                   FROM events me JOIN events o
                     ON o.id != me.id AND (o.item_id IS NULL OR o.item_id != me.item_id)
                    AND o.start < me.end AND o.end > me.start
                  WHERE me.item_id IN (SELECT value FROM json_each(?1))
                  ORDER BY me.item_id, o.start, o.id",
            )?
            .query_map([id_list(item_ids)], |r| Ok(OverlapRow { item_id: r.get(0)?, title: r.get(1)?, start: r.get(2)?, end: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn latest_rsvps(&self, item_ids: &[i64]) -> Result<Vec<AnswerRow>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                // Keyed by what was answered, so every instance of a series sees the answer.
                "WITH k AS (SELECT i.id AS item_id, i.source_id,
                                   coalesce(json_extract(i.raw_json, '$.recurringEventId'), i.external_id) AS ev
                              FROM items i WHERE i.id IN (SELECT value FROM json_each(?1)))
                 SELECT {ANSWER}
                   FROM k JOIN outbox o ON o.id = (SELECT max(o2.id) FROM outbox o2
                                                    WHERE o2.op = 'gcal.rsvp' AND o2.source_id = k.source_id
                                                      AND json_extract(o2.payload_json, '$.event_id') = k.ev)"
            ))?
            .query_map(params![id_list(item_ids)], answer)?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn latest_replies(&self, item_ids: &[i64]) -> Result<Vec<AnswerRow>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "WITH k AS (SELECT value AS item_id FROM json_each(?1))
                 SELECT {ANSWER}
                   FROM k JOIN outbox o ON o.id = (SELECT max(o2.id) FROM outbox o2 WHERE o2.op = 'slack.reply' AND o2.item_id = k.item_id)"
            ))?
            .query_map(params![id_list(item_ids)], answer)?
            .collect::<rusqlite::Result<_>>()?)
    }
}

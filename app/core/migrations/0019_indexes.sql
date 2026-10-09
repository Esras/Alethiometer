-- Indexes for the lookups the app makes on every view load and sync, which
-- were full scans (checked with EXPLAIN QUERY PLAN against a 200k-item
-- corpus; see REVIEW-2026-10-03.md, "Scale").

-- A topic's signals (topics.list, the Notes index, topic pages, merge).
CREATE INDEX topic_links_topic ON topic_links(topic_id, state);
-- Incoming: what's new. Partial, so it stays as small as the backlog.
CREATE INDEX items_unseen ON items(seen) WHERE seen = 0;
-- A calendar item's event row (every gcal upsert; foreign key checks on delete).
CREATE INDEX events_item ON events(item_id);
-- A day's events (day strip, priorities, assistant context), as a range on start.
CREATE INDEX events_start ON events(start);
-- An item's writes (foreign key checks on delete, note status).
CREATE INDEX outbox_item ON outbox(item_id) WHERE item_id IS NOT NULL;
-- Recent messages by author (new-person suggestions on every Incoming load).
CREATE INDEX item_messages_at ON item_messages(at) WHERE author_id IS NOT NULL;
-- Instances of a recurring calendar series (confirm, move, collapse, inherit).
CREATE INDEX items_series ON items(source_id, json_extract(raw_json, '$.recurringEventId'))
  WHERE json_extract(raw_json, '$.recurringEventId') IS NOT NULL;

-- Full-text index: rewrite an item's row only when its text changes, not
-- on every seen flip, status change or raw_json update.
DROP TRIGGER items_au;
CREATE TRIGGER items_au AFTER UPDATE OF title, body ON items BEGIN
  INSERT INTO items_fts(items_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
  INSERT INTO items_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;

-- Per-topic values for the Topics index, computed by the database
-- (app/docs/db-plan.md, Phase 4). Before, each list recomputed them from
-- every link of every topic.

-- When a linked item happened (its end, else its start), copied onto its
-- links and kept in step by triggers, so a topic's latest signal is one
-- index seek on (topic, state, item_at).
ALTER TABLE topic_links ADD COLUMN item_at TEXT;
UPDATE topic_links SET item_at = (SELECT coalesce(ended_at, occurred_at) FROM items WHERE id = topic_links.item_id);
CREATE INDEX topic_links_latest ON topic_links(topic_id, state, item_at);

CREATE TRIGGER topic_links_item_at AFTER INSERT ON topic_links
BEGIN
  UPDATE topic_links SET item_at = (SELECT coalesce(ended_at, occurred_at) FROM items WHERE id = NEW.item_id) WHERE rowid = NEW.rowid;
END;

CREATE TRIGGER topic_links_item_at_moved AFTER UPDATE OF item_id ON topic_links
BEGIN
  UPDATE topic_links SET item_at = (SELECT coalesce(ended_at, occurred_at) FROM items WHERE id = NEW.item_id) WHERE rowid = NEW.rowid;
END;

CREATE TRIGGER items_at_to_links AFTER UPDATE OF occurred_at, ended_at ON items
BEGIN
  UPDATE topic_links SET item_at = coalesce(NEW.ended_at, NEW.occurred_at) WHERE item_id = NEW.id;
END;

-- Open items someone has to do (a task, issue or email with an owner).
CREATE INDEX items_open_owned ON items(id) WHERE status = 'open' AND owner IS NOT NULL AND kind IN ('task', 'issue', 'email');

-- New signals per topic: unseen items with a live link. Small, as the backlog is.
CREATE VIEW topic_new_counts AS
  SELECT tl.topic_id, count(*) AS n
    FROM items i JOIN topic_links tl ON tl.item_id = i.id
   WHERE i.seen = 0 AND tl.state IN ('suggested', 'confirmed')
   GROUP BY tl.topic_id;

-- Open owned items per topic, filed there.
CREATE VIEW topic_open_counts AS
  SELECT tl.topic_id, count(*) AS n
    FROM items i JOIN topic_links tl ON tl.item_id = i.id
   WHERE i.status = 'open' AND i.owner IS NOT NULL AND i.kind IN ('task', 'issue', 'email') AND tl.state = 'confirmed'
   GROUP BY tl.topic_id;
